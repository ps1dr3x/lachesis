extern crate unindent;
extern crate serde_json;
extern crate tokio;
extern crate hyper;
extern crate hyper_tls;
extern crate futures;

use std::{
    io,
    sync::mpsc,
    time::{
        Instant,
        Duration
    },
    path::Path,
    fs::File,
    net::{
        ToSocketAddrs,
        IpAddr
    }
};
use utils::{ Definition, Options };
use easy_reader::EasyReader;

use self::tokio::timer::Timeout;
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

pub struct LacWorker {
    thread_tx: mpsc::Sender<LacResponse>,
    thread_id: u16,
    file_path: String,
    definitions: Vec<Definition>,
    targets: usize,
    debug: bool
}

impl LacWorker {
    pub fn new(
        thread_tx: mpsc::Sender<LacResponse>,
        thread_id: u16,
        file_path: String,
        definitions: Vec<Definition>,
        targets: usize,
        debug: bool
    ) -> LacWorker {
        LacWorker {
            thread_tx,
            thread_id,
            file_path,
            definitions,
            targets,
            debug
        }
    }

    pub fn run(&mut self) {
        let thread_tx = self.thread_tx.clone();
        let file_path = self.file_path.clone();
        let targets = self.targets.clone();
        let definitions = self.definitions.clone();
        let thread_id = self.thread_id.clone();
        let debug = self.debug.clone();

        rt::run(lazy(move || {
            // Open dns records file and instantiate the reader
            let dns_records_file_path = Path::new(file_path.as_str());
            let dns_records_file = File::open(dns_records_file_path).unwrap();
            let mut easy_reader = EasyReader::new(dns_records_file).unwrap();

            let mut target_n = 0;
            while target_n < targets {
                // Pick a random dns record and exclude records which are not of type A
                let line_str = easy_reader.random_line().unwrap().unwrap();
                let line_json: serde_json::Value = serde_json::from_str(&line_str).unwrap();
                if line_json["type"].as_str().unwrap() != "a" { continue; }

                let target = Target::new(line_json["name"].as_str().unwrap().to_string());

                // Check if the dns resolves the target host
                match lookup_host(target.host.as_str()) {
                    Ok(ip) => {
                        if debug { println!("New target. Host lookup: {} -> {:?}", target.host, ip); }
                    },
                    Err(err) => {
                        if debug { println!("[{}] - Host lookup failed. Error: {}", target.host, err); }
                        let mut lr = LacResponse::new(thread_id);
                        lr.unreachable = true;
                        thread_tx.send(lr).unwrap();
                        continue;
                    }
                }

                // Http/s
                let mut http_s_ports: Vec<u16> = Vec::new();
                for def in definitions.clone() {
                    if def.protocol.as_str() != "http/s" { continue; }
                    http_s_ports.append(&mut def.options.ports.clone());
                }
                LacWorker::http_s(thread_id, thread_tx.clone(), target.clone(), http_s_ports, debug);

                // Tcp/custom
                for def in definitions.clone() {
                    if def.protocol.as_str() != "tcp/custom" { continue; }
                    LacWorker::tcp_custom(
                        thread_id,
                        thread_tx.clone(),
                        target.clone(),
                        def.options.clone(),
                        debug
                    );
                }

                target_n += 1;

                if target_n == targets {
                    let mut lr = LacResponse::new(thread_id);
                    lr.last = true;
                    thread_tx.send(lr).unwrap();
                }
            }

            future::ok(())
        }));
    }

    fn http_s(
            thread_id: u16,
            thread_tx: mpsc::Sender<LacResponse>,
            target: Target,
            http_s_ports: Vec<u16>,
            debug: bool
        ) {
        for port in http_s_ports {
            let https = HttpsConnector::new(4).expect("TLS initialization failed");
            let client = Client::builder()
                .keep_alive_timeout(Duration::from_secs(1))
                .retry_canceled_requests(false)
                .build::<_, hyper::Body>(https);

            for protocol in ["https", "http"].iter() {
                let target_req = target.clone();
                let target_err = target.clone();
                let thread_tx_req = thread_tx.clone();
                let req_fut = client.get(format!("{}://{}:{}", protocol, target_req.host, port).parse().unwrap())
                    .and_then(move |res| {
                        let (parts, body) = res.into_parts();
                        body.concat2()
                            .map(move |body_content| {
                                // Merge response's headers and body
                                let mut raw_content = format!(
                                    "{:?} {}\r\n",
                                    parts.version,
                                    parts.status
                                );
                                for header in &parts.headers {
                                    raw_content = format!(
                                        "{}{}: {}\r\n",
                                        raw_content,
                                        header.0,
                                        header.1.to_str().unwrap_or("")
                                    );
                                }
                                raw_content = format!(
                                    "{}\r\n{}",
                                    raw_content,
                                    String::from_utf8(body_content.to_vec())
                                        .unwrap_or("".to_string())
                                );
                                // Send the message
                                let mut lr = LacResponse::new(thread_id);
                                lr.target.host = target_req.host;
                                lr.target.port = port;
                                lr.target.protocol = protocol.to_string();
                                lr.target.response = raw_content;
                                thread_tx_req.send(lr).unwrap();
                            })
                    })
                    .map_err(move |err| {
                        if debug {
                            println!("[{}] - {} not available. Error: {}", protocol.to_uppercase(), target_err.host, err);
                        }
                    });
                let target_timeout = target.clone();
                let req_timeout = Timeout::new(req_fut, Duration::from_secs(5))
                    .map_err(move |_err| {
                        if debug {
                            println!("[{}] - Timeout reached ({})", target_timeout.host, protocol.to_uppercase());
                        }
                    });
                rt::spawn(req_timeout);
            }
        }
    }

    fn tcp_custom(
            thread_id: u16,
            thread_tx: mpsc::Sender<LacResponse>,
            target: Target,
            options: Options,
            debug: bool
        ) {
        use std::net::{ TcpStream, SocketAddr, ToSocketAddrs };
        use std::io::{ Error, Read, Write };

        for port in options.ports {
            let host = target.host.clone();

            let addr = format!("{}:{}", host, port).to_socket_addrs();
            if addr.is_err() {
                if debug {
                    println!("[{}:{}] - TCP stream connection error: {}\n", host, port, addr.err().unwrap());
                }
                continue;
            }

            let mut addr: Vec<SocketAddr> = addr.unwrap().collect();
            let addr = addr.pop().unwrap();

            let stream: Result<TcpStream, Error> = TcpStream::connect_timeout(&addr, Duration::from_secs(5));
            if stream.is_err() {
                if debug {
                    println!("[{}:{}] - TCP stream connection error: {}\n", host, port, stream.err().unwrap());
                }
                continue;
            }
            let mut stream: TcpStream = stream.unwrap();

            stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let stream_write: Result<(), Error> = stream.write_all(options.message.clone().unwrap().as_bytes());
            if stream_write.is_err() {
                if debug {
                    println!("[{}:{}] - TCP stream write error: {}\n", host, port, stream_write.err().unwrap());
                }
                continue;
            }

            let start = Instant::now();
            let mut res_string: String = String::new();
            if options.timeout.unwrap_or(true) {
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

                while start.elapsed().as_secs() < 5 {
                    let mut buf = [0];
                    match stream.read(&mut buf) {
                        Err(e) => {
                            if res_string.len() > 0 { break; }
                            if debug {
                                println!("[{}:{}] - TCP stream read error: {}\n", host, port, e);
                            }
                            break;
                        },
                        Ok(m) => {
                            if m == 0 {
                                if debug {
                                    println!("[{}:{}] - TCP stream read error: empty response\n", host, port);
                                }
                                break;
                            }
                            res_string += String::from_utf8(buf.to_vec()).unwrap().as_str();
                        },
                    };
                }
            } else {
                if stream.read_to_string(&mut res_string).unwrap_or(0) == 0 {
                    if debug {
                        println!("[{}:{}] - TCP stream read error: empty response\n", host, port);
                    }
                    continue;
                }
            }

            if !res_string.is_empty() {
                let mut lr = LacResponse::new(thread_id);
                lr.target.host = host;
                lr.target.port = port;
                lr.target.protocol = "tcp/custom".to_string();
                lr.target.response = res_string;
                thread_tx.send(lr).unwrap();
            }
        }
    }
}

pub fn lookup_host(host: &str) -> io::Result<Vec<IpAddr>> {
    (host, 0).to_socket_addrs().map(|iter| iter.map(|socket_address| socket_address.ip()).collect())
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
