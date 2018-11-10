extern crate reqwest;
extern crate unindent;
extern crate serde_json;
extern crate hyper;
extern crate hyper_tls;
extern crate futures;

use std::{
    thread,
    sync::mpsc,
    time::{
        Instant,
        Duration
    },
    path::Path,
    fs::File
};
use utils::Definition;
use easy_reader::EasyReader;

use self::futures::{ future, lazy };
use self::hyper::{
    Client,
    rt::{
        self,
        Future,
        Stream
    }
};
use self::hyper_tls::HttpsConnector;

#[derive(Debug)]
pub struct LacResponse {
    pub thread_id: u16,
    pub unreachable: bool,
    pub last: bool,
    pub target: Target
}

impl LacResponse {
    fn default() -> LacResponse {
        LacResponse {
            thread_id: 0,
            unreachable: false,
            last: false,
            target: Target::default()
        }
    }

    fn new(thread_id: u16) -> LacResponse {
        LacResponse {
            thread_id: thread_id,
            ..LacResponse::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub response: String
}

impl Target {
    fn default() -> Target {
        Target {
            host: "".to_string(),
            port: 0,
            protocol: "".to_string(),
            response: "".to_string()
        }
    }

    fn new(target: String) -> Target {
        Target {
            host: target,
            ..Target::default()
        }
    }
}

pub fn lac_worker(
        thread_tx: mpsc::Sender<LacResponse>,
        thread_id: u16,
        file_path: String,
        definitions: Vec<Definition>,
        requests: usize,
        debug: bool
    ) -> thread::JoinHandle<()> {
    if debug { println!("Spawning new worker. ID: {}", thread_id); }

    thread::spawn(move || {
        let out_tx = thread_tx.clone();

        rt::run(lazy(move || {
            // Open dns records file and instantiate the reader
            let dns_records_file_path = Path::new(file_path.as_str());
            let dns_records_file = File::open(dns_records_file_path).unwrap();
            let mut easy_reader = EasyReader::new(dns_records_file).unwrap();

            let mut req = 0;
            while req < requests {
                // Pick a random dns record and exclude records which are not of type A
                let line_str = easy_reader.random_line().unwrap();
                let line_json: serde_json::Value = serde_json::from_str(&line_str).unwrap();
                if line_json["type"].as_str().unwrap() != "a" { continue; }

                let target = Target::new(line_json["name"].as_str().unwrap().to_string());

                // Http/s
                let mut http_s_ports: Vec<u16> = Vec::new();
                for def in definitions.clone() {
                    if def.protocol.as_str() != "http/s" { continue; }

                    let mut options = def.options.unwrap();
                    http_s_ports.append(&mut options.ports);
                }
                http_s(thread_id, thread_tx.clone(), target.clone(), http_s_ports, debug);

                // Tcp/custom
                for def in definitions.clone() {
                    if def.protocol.as_str() != "tcp/custom" { continue; }

                    let options = def.options.unwrap();
                    if options.message.is_none() {
                        println!("[ERROR] Missing mandatory option for protocol tcp/custom: 'message'. Service: {}\n", def.name);
                        ::std::process::exit(1);
                    }
                    tcp_custom(
                        thread_id,
                        thread_tx.clone(),
                        target.clone(),
                        options.ports,
                        options.message.clone().unwrap().as_str(),
                        options.timeout.unwrap_or(true),
                        debug
                    );
                }

                req += 1;
            }

            future::ok(())
        }));

        let mut lr = LacResponse::new(thread_id);
        lr.last = true;
        out_tx.send(lr).unwrap();
    })
}

fn http_s(
        thread_id: u16,
        thread_tx: mpsc::Sender<LacResponse>,
        mut target: Target,
        http_s_ports: Vec<u16>,
        debug: bool
    ) {
    for port in http_s_ports {
        let thread_tx_https_ok = thread_tx.clone();
        let thread_tx_http_ok = thread_tx.clone();
        let thread_tx_err = thread_tx.clone();

        target.port = port;
        let target_https = target.clone();
        let target_http = target.clone();

        if debug { println!("New target: {}", target.host); }

        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        let client = Client::builder()
            .keep_alive(false)
            .retry_canceled_requests(false)
            .build::<_, hyper::Body>(https);

        let req_fut = client.get(format!("https://{}:{}", target_https.host, port).parse().unwrap())
            .and_then(|res| {
                res.into_body().concat2()
            })
            .map(move |content| {
                // TODO - Add headers
                let mut lr = LacResponse::new(thread_id);
                lr.target.protocol = "https".to_string();
                lr.target.response = String::from_utf8(content.to_vec()).unwrap_or("".to_string());
                thread_tx_https_ok.send(lr).unwrap();
            })
            .map_err(move |err| {
                if debug { 
                    println!("[{}] - HTTPS not available. Error: {}", target_https.host, err);
                    println!("[{}] - Trying plain HTTP...", target_https.host)
                }
                let req_fut = client.get(format!("http://{}:{}", target_http.host, port).parse().unwrap())
                    .and_then(|res| {
                        res.into_body().concat2()
                    })
                    .map(move |content| {
                        let mut lr = LacResponse::new(thread_id);
                        lr.target.protocol = "http".to_string();
                        lr.target.response = String::from_utf8(content.to_vec()).unwrap_or("".to_string());
                        thread_tx_http_ok.send(lr).unwrap();
                    })
                    .map_err(move |err| {
                        if debug {
                            println!("[{}] - HTTP request error: {}", target_http.host, err);
                        }
                        let mut lr = LacResponse::new(thread_id);
                        lr.unreachable = true;
                        thread_tx_err.send(lr).unwrap();
                    });
                rt::spawn(req_fut);
            });
        rt::spawn(req_fut);
    }
}

pub fn tcp_custom(
        thread_id: u16,
        thread_tx: mpsc::Sender<LacResponse>,
        mut target: Target,
        ports: Vec<u16>,
        message: &str,
        timeout: bool,
        debug: bool
    ) {
    use std::net::{ TcpStream, SocketAddr, ToSocketAddrs };
    use std::io::{ Error, Read, Write };

    for port in ports {
        let mut lr = LacResponse::new(thread_id);

        target.port = port;

        let addr = format!("{}:{}", target.host, port).to_socket_addrs();
        if addr.is_err() {
            if debug {
                println!("[{}:{}] - TCP stream connection error: {}\n", target.host, port, addr.err().unwrap());
            }
            lr.unreachable = true;
            thread_tx.send(lr).unwrap();
            continue;
        }

        let mut addr: Vec<SocketAddr> = addr.unwrap().collect();
        let addr = addr.pop().unwrap();

        let stream: Result<TcpStream, Error> = TcpStream::connect_timeout(&addr, Duration::from_secs(5));
        if stream.is_err() {
            if debug {
                println!("[{}:{}] - TCP stream connection error: {}\n", target.host, port, stream.err().unwrap());
            }
            lr.unreachable = true;
            thread_tx.send(lr).unwrap();
            continue;
        }
        let mut stream: TcpStream = stream.unwrap();

        stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        let stream_write: Result<(), Error> = stream.write_all(message.as_bytes());
        if stream_write.is_err() {
            if debug {
                println!("[{}:{}] - TCP stream write error: {}\n", target.host, port, stream_write.err().unwrap());
            }
            lr.unreachable = true;
            thread_tx.send(lr).unwrap();
            continue;
        }

        let start = Instant::now();
        let mut res_string: String = String::new();
        if timeout {
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

            while start.elapsed().as_secs() < 5 {
                let mut buf = [0];
                match stream.read(&mut buf) {
                    Err(e) => {
                        if res_string.len() > 0 { break; }
                        if debug {
                            println!("[{}:{}] - TCP stream read error: {}\n", target.host, port, e);
                        }
                        lr.unreachable = true;
                        thread_tx.send(lr).unwrap();
                        break;
                    },
                    Ok(m) => {
                        if m == 0 {
                            if debug {
                                println!("[{}:{}] - TCP stream read error: empty response\n", target.host, port);
                            }
                            lr.unreachable = true;
                            thread_tx.send(lr).unwrap();
                            break;
                        }
                        res_string += String::from_utf8(buf.to_vec()).unwrap().as_str();
                    },
                };
            }
        } else {
            if stream.read_to_string(&mut res_string).unwrap_or(0) == 0 {
                if debug {
                    println!("[{}:{}] - TCP stream read error: empty response\n", target.host, port);
                }
                lr.unreachable = true;
                thread_tx.send(lr).unwrap();
                continue;
            }
        }
    }
}

#[allow(dead_code)]
fn ip2hex(ip: &str) -> u32 {
    let parts = ip.split('.').map(|p| p.parse::<u32>().unwrap());

    let mut n: u32 = 0;

    for (idx, p) in parts.enumerate() {
        match idx {
            3 => n += p,
            2 => n += p * 256,        // 2^8
            1 => n += p * 65536,      // 2^16
            0 => n += p * 16777216,   // 2^24
            _ => println!("?"),
        }
    }

    n
}

#[allow(dead_code)]
pub fn ip_range(ip1: &str, ip2: &str) {
    let mut hex: u32 = ip2hex(ip1);
    let mut hex2: u32 = ip2hex(ip2);

    if hex > hex2 {
        let tmp = hex;
        hex = hex2;
        hex2 = tmp;
    }

    let mut i: u32 = hex;
    while i <= hex2 {
        println!("{}", format!("{}.{}.{}.{}", i >> 24 & 0xff, i >> 16 & 0xff, i >> 8 & 0xff, i & 0xff));
        i += 1
    }
}

#[allow(dead_code)]
pub fn get(host: &str, port: u16, path: &str) -> Result<String, String> {
    use std::net::TcpStream;
    use std::io::{Error, Read, Write};

    let addr: String = format!("{}:{}", host, port);

    let stream: Result<TcpStream, Error> = TcpStream::connect(&addr);
    if stream.is_err() {
        return Err(format!("Stream connect error: \n{}\n", stream.err().unwrap()))
    }
    let mut stream: TcpStream = stream.unwrap();

    let header = format!("GET {} HTTP/1.1\r\n Host: {} \r\n User-Agent: h3ist/6.6.6 \r\n Accept: */* \r\n\r\n", path, addr);

    let stream_write: Result<(), Error> = stream.write_all(header.as_bytes());
    if stream_write.is_err() {
        return Err(format!("Stream write error: \n{}\n", stream_write.err().unwrap()))
    }

    let mut res_string: String = String::new();
    if stream.read_to_string(&mut res_string).unwrap() == 0 {
        return Err(format!("Stream read error: \nempty response\n"));
    }

    Ok(res_string)
}
