extern crate reqwest;
extern crate unindent;

use std::{
    thread,
    sync::mpsc,
    time::{
        Instant,
        Duration
    }
};
use utils::Definition;
use detector::Detector;
use db::DbMan;
use unindent::unindent;

#[derive(Debug)]
pub struct LacResponse {
    pub thread_id: u16,
    pub unreachable: bool,
    pub https: Vec<u16>,
    pub http: Vec<u16>,
    pub tcp_custom: Vec<u16>,
    pub matching: u16
}

pub fn lac_request_thread(
        thread_tx: mpsc::Sender<LacResponse>,
        thread_id: u16,
        definitions: Vec<Definition>,
        target: String,
        debug: bool
    ) -> thread::JoinHandle<()> {
    if debug { println!("[{}] - Spawning new thread. ID: {}\n", target, thread_id); }

    thread::spawn(move || {
        let mut wr: LacResponse = LacResponse {
            thread_id: thread_id,
            unreachable: false,
            https: Vec::new(),
            http: Vec::new(),
            tcp_custom: Vec::new(),
            matching: 0
        };
        let mut responses: Vec<(u16, String)> = Vec::new();

        // Http/s
        let mut http_s_ports: Vec<u16> = Vec::new();
        for def in definitions.clone() {
            if def.protocol.as_str() != "http/s" { continue; }

            let mut options = def.options.unwrap();
            http_s_ports.append(&mut options.ports);
        }

        for port in http_s_ports {
            let mut url: String = format!("https://{}:{}", target, port);
            let mut response = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(url.as_str())
                .send();

            if response.is_ok() { wr.https.push(port); }
            if response.is_err() {
                if debug { 
                    println!("[{}] - HTTPS not available\nRequest error: {}\n", target, response.unwrap_err()); 
                    println!("[{}] - Trying plain HTTP...\n", target)
                }
                url = format!("http://{}:{}", target, port);
                response = reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()
                    .unwrap()
                    .get(url.as_str())
                    .send();

                if response.is_ok() { wr.http.push(port); }
            }
            if response.is_ok() {
                let mut response = response.unwrap();
                match response.text() {
                    Err(e) => {
                        println!("[{}] - HTTP response unwrap error: {}\n", target, e);
                    },
                    Ok(text) => {
                        responses.push((
                            port,
                            format!("{}\r\n\r\n{}", response.headers(), text)
                        ));
                    },
                };
            } else if debug { println!("[{}] - HTTP request error: {}\n", target, response.unwrap_err()); }
        }

        // Tcp/custom
        for def in definitions.clone() {
            if def.protocol.as_str() != "tcp/custom" { continue; }

            let options = def.options.unwrap();
            if options.message.is_none() {
                println!("[ERROR] Missing mandatory option for protocol tcp/custom: 'message'. Service: {}\n", def.name);
                ::std::process::exit(1);
            }

            for port in options.ports {
                let response = tcp_custom(
                    target.as_str(),
                    port,
                    options.message.clone().unwrap().as_str(),
                    options.timeout.unwrap_or(true)
                );
                
                if response.is_ok() {
                    wr.tcp_custom.push(port);
                    responses.push((
                        port,
                        response.unwrap()
                    ));
                } else if debug { println!("{}", response.unwrap_err()); }
            }
        }

        // Check if there has been at least one successful connection
        if wr.https.is_empty() && wr.http.is_empty() && wr.tcp_custom.is_empty() {
            wr.unreachable = true;
            return thread_tx.send(wr).unwrap();
        }

        // Detection
        for res in responses {
            let mut detector: Detector = Detector::new(definitions.clone());
            detector.run(
                target.to_string(),
                res.0,
                res.1
            );

            if !detector.response.is_empty() {
                wr.matching = detector.response.len() as u16;

                for res in detector.response {
                    println!("{}", unindent(format!("
                        ===
                        Matching service found: {}
                        Service: {}
                        Version: {}
                        ===
                    ",
                        target,
                        res.service,
                        res.version).as_str())
                    );

                    let dbm: DbMan = DbMan::new();
                    dbm.save_service(res).unwrap();
                }
            }
        }

        // Send result message
        thread_tx.send(wr).unwrap();
    })
}

pub fn tcp_custom(host: &str, port: u16, message: &str, timeout: bool) -> Result<String, String> {
    use std::net::{ TcpStream, SocketAddr, ToSocketAddrs };
    use std::io::{ Error, Read, Write };

    let addr = format!("{}:{}", host, port).to_socket_addrs();
    if addr.is_err() {
        return Err(format!("[{}:{}] - TCP stream connection error: \n{}\n", host, port, addr.err().unwrap()));
    }

    let mut addr: Vec<SocketAddr> = addr.unwrap().collect();
    let addr = addr.pop().unwrap();

    let stream: Result<TcpStream, Error> = TcpStream::connect_timeout(&addr, Duration::from_secs(5));
    if stream.is_err() {
        return Err(format!("[{}:{}] - TCP stream connection error: \n{}\n", host, port, stream.err().unwrap()));
    }
    let mut stream: TcpStream = stream.unwrap();

    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let stream_write: Result<(), Error> = stream.write_all(message.as_bytes());
    if stream_write.is_err() {
        return Err(format!("[{}:{}] - TCP stream write error: \n{}\n", host, port, stream_write.err().unwrap()));
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
                    return Err(format!("[{}:{}] - TCP stream read error: {}\n", host, port, e));
                },
                Ok(m) => {
                    if m == 0 {
                        return Err(format!("[{}:{}] - TCP stream read error: \nempty response\n", host, port));
                    }
                    res_string += String::from_utf8(buf.to_vec()).unwrap().as_str();
                },
            };
        }
    } else {
        if stream.read_to_string(&mut res_string).unwrap_or(0) == 0 {
            return Err(format!("[{}:{}] - TCP stream read error: \nempty response\n", host, port));
        }
    }

    Ok(res_string)
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
