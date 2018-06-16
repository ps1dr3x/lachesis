extern crate reqwest;
extern crate unindent;

use std::thread;
use std::sync::mpsc;
use detector::Detector;
use db::DbMan;
use unindent::unindent;

pub struct LacResponse {
    pub thread_id: u16,
    pub is_request_error: bool,
    pub is_https: bool,
    pub is_http: bool,
    pub is_status_not_ok: bool,
    pub matching: u16
}

pub fn lac_request_thread(debug: bool, thread_tx: mpsc::Sender<LacResponse>, thread_id: u16, target: String) -> thread::JoinHandle<()> {
    if debug { println!("Spawning new thread. ID: {}\nTarget: {}\n", thread_id, target); }
    thread::spawn(move || {
        let mut wr: LacResponse = LacResponse {
            thread_id: thread_id,
            is_request_error: false,
            is_https: false,
            is_http: false,
            is_status_not_ok: false,
            matching: 0
        };

        let mut url: String = format!("https://{}", target);
        let mut response: reqwest::Response;
        match reqwest::get(url.as_str()) {
            Ok(r) => response = r,
            Err(e) => {
                if debug { println!("HTTPS not available on target: {}\n Request error: {}\n Trying plain http...\n", target, e); }
                wr.is_https = true;

                url = format!("http://{}", target);
                match reqwest::get(url.as_str()) {
                    Ok(r) => {
                        response = r;
                        wr.is_http = true;
                    },
                    Err(_e) => {
                        if debug { println!("HTTP request error: {}\n", e); }
                        wr.is_request_error = true;
                        thread_tx.send(wr).unwrap();
                        return;
                    }
                }
            }
        }

        if response.status() != reqwest::StatusCode::Ok {
            if debug { println!("Request status not OK: {}\n", response.status()); }
            wr.is_status_not_ok = true;
        }

        let response_text: String = response.text().unwrap_or("Error".to_string());
        if !response_text.eq("Error") {
            // Valid response body. Run the detector
            let mut detector: Detector = Detector::new();
            detector.run(
                target.to_string(),
                if wr.is_http { 80 } else { 443 },
                response_text
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

                    // Save on db
                    let dbm: DbMan = DbMan::new();
                    dbm.save_service(res).unwrap();
                }
            }
        }

        // Send result message
        thread_tx.send(wr).unwrap();
    })
}

pub fn tcp(host: &str, port: u16, message: &str, timeout: bool) -> Result<String, String> {
    use std::net::TcpStream;
    use std::io::{Error, Read, Write};
    use std::time::Duration;

    let addr: String = format!("{}:{}", host, port);

    let stream: Result<TcpStream, Error> = TcpStream::connect(&addr);
    if stream.is_err() {
        return Err(format!("Stream connection error: \n{}\n", stream.err().unwrap()))
    }
    let mut stream: TcpStream = stream.unwrap();

    let stream_write: Result<(), Error> = stream.write_all(message.as_bytes());
    if stream_write.is_err() {
        return Err(format!("Stream write error: \n{}\n", stream_write.err().unwrap()))
    }

    let mut res_string: String = String::new();
    if timeout {
        stream.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        loop {
            let mut buf = [0];
            match stream.read(&mut buf) {
                Err(e) => {
                    if res_string.len() > 0 { break; }
                    return Err(format!("TCP stream read error: {}\n", e));
                },
                Ok(m) => {
                    if m == 0 {
                        return Err("TCP stream read error: \nempty response\n".to_string());
                    }
                    res_string += String::from_utf8(buf.to_vec()).unwrap().as_str();
                },
            };
        }
    } else {
        if stream.read_to_string(&mut res_string).unwrap() == 0 {
            return Err("TCP stream read error: \nempty response\n".to_string());
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
