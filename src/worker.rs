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
use easy_reader::EasyReader;
use lachesis::{ Definition, Options };
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

#[derive(Debug, Clone)]
pub struct LacMessage {
    pub thread_id: u16,
    pub message: String,
    unreachable: bool,
    next_target: bool,
    pub target: Target,
    last_message: bool
}

impl LacMessage {
    fn default() -> LacMessage {
        LacMessage {
            thread_id: 0,
            message: "".to_string(),
            unreachable: false,
            next_target: false,
            target: Target::default(),
            last_message: false
        }
    }

    fn new(thread_id: u16) -> LacMessage {
        LacMessage {
            thread_id: thread_id,
            ..LacMessage::default()
        }
    }

    fn new_log(thread_id: u16, message: String) -> LacMessage {
        LacMessage {
            thread_id: thread_id,
            message: message,
            ..LacMessage::default()
        }
    }

    pub fn is_log(&self) -> bool {
        !self.message.is_empty()
    }

    pub fn is_unreachable(&self) -> bool {
        self.unreachable
    }

    pub fn is_next_target_message(&self) -> bool {
        self.next_target
    }

    pub fn is_last_message(&self) -> bool {
        self.last_message
    }
}

pub struct LacWorker {
    thread_tx: mpsc::Sender<LacMessage>,
    thread_id: u16,
    dataset: String,
    definitions: Vec<Definition>,
    targets: usize
}

impl LacWorker {
    pub fn new(
        thread_tx: mpsc::Sender<LacMessage>,
        thread_id: u16,
        dataset: String,
        definitions: Vec<Definition>,
        targets: usize
    ) -> LacWorker {
        LacWorker {
            thread_tx,
            thread_id,
            dataset,
            definitions,
            targets
        }
    }

    pub fn run(&mut self) {
        let thread_tx = self.thread_tx.clone();
        let dataset = self.dataset.clone();
        let targets = self.targets.clone();
        let definitions = self.definitions.clone();
        let thread_id = self.thread_id.clone();

        rt::run(lazy(move || {
            // Open dataset and instantiate the reader
            let dataset_path = Path::new(dataset.as_str());
            let dataset_file = File::open(dataset_path).unwrap();
            let mut easy_reader = EasyReader::new(dataset_file).unwrap();

            let mut target_n = 0;
            while target_n < targets {
                // Pick a random dns record and exclude records which are not of type A
                let line_str = easy_reader.random_line().unwrap().unwrap();
                let line_json: serde_json::Value = serde_json::from_str(&line_str).unwrap();
                if line_json["type"].as_str().unwrap() != "a" { continue; }

                let mut lr = LacMessage::new(thread_id);
                lr.target = Target::new(line_json["name"].as_str().unwrap().to_string());

                // Check if the dns resolves the target host
                match lookup_host(lr.target.host.as_str()) {
                    Ok(ip) => {
                        let msg = LacMessage::new_log(
                            thread_id,
                            format!("New target. Host lookup: {} -> {:?}", lr.target.host, ip)
                        );
                        thread_tx.send(msg).unwrap();
                    },
                    Err(err) => {
                        let msg = LacMessage::new_log(
                            thread_id,
                            format!("[{}:{}] - Host lookup failed. Error: {}", lr.target.host, lr.target.port, err)
                        );
                        thread_tx.send(msg).unwrap();
                        lr.unreachable = true;
                        thread_tx.send(lr).unwrap();
                        continue;
                    }
                }

                // Http/s
                for def in definitions.clone() {
                    if def.protocol.as_str() != "http/s" { continue; }
                    LacWorker::http_s(
                        thread_id,
                        thread_tx.clone(),
                        lr.target.clone(),
                        def.options.clone()
                    );
                }

                // Tcp/custom
                for def in definitions.clone() {
                    if def.protocol.as_str() != "tcp/custom" { continue; }
                    LacWorker::tcp_custom(
                        thread_id,
                        thread_tx.clone(),
                        lr.target.clone(),
                        def.options.clone()
                    );
                }

                // Last request for this target
                lr.next_target = true;
                target_n += 1;

                // Last message of the worker
                if target_n == targets {
                    lr.last_message = true;
                }

                thread_tx.send(lr).unwrap();
            }

            future::ok(())
        }));
    }

    fn http_s(
            thread_id: u16,
            thread_tx: mpsc::Sender<LacMessage>,
            target: Target,
            options: Options
        ) {
        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        let client = Client::builder()
            .keep_alive_timeout(Duration::from_secs(1))
            .retry_canceled_requests(false)
            .build::<_, hyper::Body>(https);

        for protocol in ["https", "http"].iter() {
            for port in &options.ports {
                let mut target_req = target.clone();
                target_req.protocol = protocol.to_string();
                target_req.port = port.clone();

                let target_err = target_req.clone();
                let target_timeout = target_req.clone();
                let thread_tx_req = thread_tx.clone();
                let thread_tx_err = thread_tx.clone();
                let thread_tx_timeout = thread_tx.clone();
                let req_fut = client.get(format!("{}://{}:{}", target_req.protocol, target_req.host, target_req.port).parse().unwrap())
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
                                    String::from_utf8_lossy(&body_content.to_vec())
                                );
                                target_req.response = raw_content;
                                // Send the message
                                let mut lr = LacMessage::new(thread_id);
                                lr.target = target_req;
                                thread_tx_req.send(lr).unwrap();
                            })
                    })
                    .map_err(move |err| {
                        let msg = LacMessage::new_log(
                            thread_id,
                            format!(
                                "[{}:{}] - {} not available. Error: {}",
                                target_err.protocol.to_uppercase(),
                                target_err.port,
                                target_err.host,
                                err
                            )
                        );
                        thread_tx_err.send(msg).unwrap();
                    });
                let req_timeout = Timeout::new(req_fut, Duration::from_secs(5))
                    .map_err(move |_err| {
                        let msg = LacMessage::new_log(
                            thread_id,
                            format!(
                                "[{}:{}] - Timeout reached ({})",
                                target_timeout.host,
                                target_timeout.port,
                                target_timeout.protocol.to_uppercase()
                            )
                        );
                        thread_tx_timeout.send(msg).unwrap();
                    });
                rt::spawn(req_timeout);
            }
        }
    }

    fn tcp_custom(
            thread_id: u16,
            thread_tx: mpsc::Sender<LacMessage>,
            target: Target,
            options: Options
        ) {
        use std::net::{ TcpStream, SocketAddr, ToSocketAddrs };
        use std::io::{ Error, Read, Write };

        for port in options.ports {
            let host = target.host.clone();

            let addr = format!("{}:{}", host, port).to_socket_addrs();
            if addr.is_err() {
                let msg = LacMessage::new_log(
                    thread_id,
                    format!(
                        "[{}:{}] - TCP stream connection error: {}\n",
                        host,
                        port,
                        addr.err().unwrap()
                    )
                );
                thread_tx.send(msg).unwrap();
                continue;
            }

            let mut addr: Vec<SocketAddr> = addr.unwrap().collect();
            let addr = addr.pop().unwrap();

            let stream: Result<TcpStream, Error> = TcpStream::connect_timeout(&addr, Duration::from_secs(5));
            if stream.is_err() {
                let msg = LacMessage::new_log(
                    thread_id,
                    format!(
                        "[{}:{}] - TCP stream connection error: {}\n",
                        host,
                        port,
                        stream.err().unwrap()
                    )
                );
                thread_tx.send(msg).unwrap();
                continue;
            }
            let mut stream: TcpStream = stream.unwrap();

            stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
            let stream_write: Result<(), Error> = stream.write_all(options.message.clone().unwrap().as_bytes());
            if stream_write.is_err() {
                let msg = LacMessage::new_log(
                    thread_id,
                    format!(
                        "[{}:{}] - TCP stream write error: {}\n",
                        host,
                        port,
                        stream_write.err().unwrap()
                    )
                );
                thread_tx.send(msg).unwrap();
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
                            let msg = LacMessage::new_log(
                                thread_id,
                                format!("[{}:{}] - TCP stream read error: {}\n", host, port, e)
                            );
                            thread_tx.send(msg).unwrap();
                            break;
                        },
                        Ok(m) => {
                            if m == 0 {
                                let msg = LacMessage::new_log(
                                    thread_id,
                                    format!("[{}:{}] - TCP stream read error: empty response\n", host, port)
                                );
                                thread_tx.send(msg).unwrap();
                                break;
                            }
                            res_string += String::from_utf8(buf.to_vec()).unwrap().as_str();
                        },
                    };
                }
            } else {
                if stream.read_to_string(&mut res_string).unwrap_or(0) == 0 {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!("[{}:{}] - TCP stream read error: empty response\n", host, port)
                    );
                    thread_tx.send(msg).unwrap();
                    continue;
                }
            }

            if !res_string.is_empty() {
                let mut lr = LacMessage::new(thread_id);
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
