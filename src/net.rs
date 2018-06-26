extern crate reqwest;
extern crate unindent;

use std::{
    thread,
    sync::mpsc,
    time::Instant
};
use detector::Detector;
use db::DbMan;
use unindent::unindent;

pub struct LacResponse {
    pub thread_id: u16,
    pub is_request_error: bool,
    pub is_https: bool,
    pub is_http: bool,
    pub is_tcp_custom: bool,
    pub matching: u16
}

pub fn lac_request_thread(thread_tx: mpsc::Sender<LacResponse>, thread_id: u16, target: String, debug: bool) -> thread::JoinHandle<()> {
    if debug { println!("[{}] - Spawning new thread. ID: {}\n", target, thread_id); }

    thread::spawn(move || {
        let mut wr: LacResponse = LacResponse {
            thread_id: thread_id,
            is_request_error: false,
            is_https: false,
            is_http: false,
            is_tcp_custom: false,
            matching: 0
        };
        let mut responses: Vec<(u16, String)> = Vec::new();

        // Http/s
        let mut url: String = format!("https://{}", target);
        let mut response = reqwest::get(url.as_str());
        if response.is_ok() { wr.is_https = true; }
        if response.is_err() {
            if debug { 
                println!("[{}] - HTTPS not available\nRequest error: {}\n", target, response.unwrap_err()); 
                println!("[{}] - Trying plain HTTP...\n", target)
            }
            url = format!("http://{}", target);
            response = reqwest::get(url.as_str());
            if response.is_ok() { wr.is_http = true; }
        }
        if response.is_ok() {
            responses.push((
                if wr.is_https { 443 } else { 80 },
                response.unwrap().text().unwrap()
            ));
        } else if debug { println!("[{}] - HTTP request error: {}\n", target, response.unwrap_err()); }

        // Tcp/custom
        let definitions = super::utils::read_definitions().unwrap();
        for def in definitions {
            if def.protocol.as_str() != "tcp/custom" {
                continue;
            }

            let options = def.options.unwrap();
            for port in options.ports {
                let response = tcp_custom(
                    target.as_str(),
                    port,
                    options.message.as_str(),
                    options.timeout
                );
                
                if response.is_ok() {
                    wr.is_tcp_custom == true;
                    responses.push((
                        port,
                        response.unwrap()
                    ));
                } else if debug { println!("{}", response.unwrap_err()); }
            }
        }

        // Check if there has been at least one successful connection
        if !wr.is_https && !wr.is_http && !wr.is_tcp_custom {
            wr.is_request_error = true;
            return thread_tx.send(wr).unwrap();
        }

        // Detection
        for res in responses {
            let mut detector: Detector = Detector::new();
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
    use std::net::TcpStream;
    use std::io::{Error, Read, Write};
    use std::time::Duration;

    let addr: String = format!("{}:{}", host, port);

    let stream: Result<TcpStream, Error> = TcpStream::connect(&addr);
    if stream.is_err() {
        return Err(format!("[{}:{}] - TCP stream connection error: \n{}\n", host, port, stream.err().unwrap()))
    }
    let mut stream: TcpStream = stream.unwrap();

    let stream_write: Result<(), Error> = stream.write_all(message.as_bytes());
    if stream_write.is_err() {
        return Err(format!("[{}:{}] - TCP stream write error: \n{}\n", host, port, stream_write.err().unwrap()))
    }

    let start = Instant::now();
    let mut res_string: String = String::new();
    if timeout {
        stream.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

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
