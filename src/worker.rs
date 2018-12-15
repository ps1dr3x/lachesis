use std::{
    sync::mpsc,
    time::Duration,
    path::Path,
    fs::File,
    net::SocketAddr,
    io::BufReader
};
use serde_derive::{
    Serialize,
    Deserialize
};
use futures::{
    future,
    lazy
};
use tokio::{
    io,
    net::TcpStream,
    timer::Timeout
};
use hyper::{
    Client,
    rt::{
        self,
        Future,
        Stream
    }
};
use hyper_tls::HttpsConnector;
use easy_reader::EasyReader;
use colored::Colorize;
use crate::lachesis::{
    LacConf,
    Options
};
use crate::utils;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String
}

#[derive(Debug, Clone)]
pub struct Target {
    pub domain: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub response: String
}

impl Target {
    fn default() -> Target {
        Target {
            domain: "".to_string(),
            ip: "".to_string(),
            port: 0,
            protocol: "".to_string(),
            response: "".to_string()
        }
    }

    fn new(domain: String, ip: String) -> Self {
        Target {
            domain,
            ip,
            ..Target::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct LacMessage {
    pub thread_id: u16,
    pub message: String,
    next_target: bool,
    pub target: Target,
    last_message: bool
}

impl LacMessage {
    fn default() -> LacMessage {
        LacMessage {
            thread_id: 0,
            message: "".to_string(),
            next_target: false,
            target: Target::default(),
            last_message: false
        }
    }

    fn new(thread_id: u16) -> Self {
        LacMessage {
            thread_id,
            ..LacMessage::default()
        }
    }

    fn new_log(thread_id: u16, message: String) -> Self {
        LacMessage {
            thread_id,
            message,
            ..LacMessage::default()
        }
    }

    pub fn is_log(&self) -> bool {
        !self.message.is_empty()
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
    conf: LacConf,
    targets: usize
}

impl LacWorker {
    pub fn new(
        thread_tx: mpsc::Sender<LacMessage>,
        thread_id: u16,
        conf: LacConf,
        targets: usize
    ) -> Self {
        LacWorker {
            thread_tx,
            thread_id,
            conf,
            targets
        }
    }

    pub fn run(&mut self) {
        // Clone and move the necessary objects and start the runtime
        let targets = self.targets;
        let thread_tx = self.thread_tx.clone();
        let conf = self.conf.clone();
        let thread_id = self.thread_id;
        rt::run(lazy(move || {
            let mut target_n = 0;
            while !conf.with_limit || target_n < targets {
                let mut lr = LacMessage::new(thread_id);
                lr.target = if !conf.dataset.is_empty() {
                    // If dataset mode open and instantiate the reader
                    let dataset_path = Path::new(conf.dataset.as_str());
                    let dataset_file = File::open(dataset_path).unwrap();
                    let mut easy_reader = EasyReader::new(dataset_file).unwrap();
                    // Pick a random dns record and exclude records which are not of type A
                    let line_str = easy_reader.random_line().unwrap().unwrap();
                    let dataset_record: DatasetRecord = serde_json::from_str(&line_str).unwrap();
                    if dataset_record.record_type != "a" { continue; }
                    Target::new(dataset_record.name, dataset_record.value)
                } else {
                    // Pick a random ip in the specified range
                    let random_ip = utils::random_ip_in_range(&conf.ip_range.0, &conf.ip_range.1).unwrap();
                    Target::new(random_ip.clone(), random_ip)
                };

                // Requests
                for def in &conf.definitions {
                    match def.protocol.as_str() {
                        "http/s" => {
                            LacWorker::http_s(
                                thread_id,
                                &thread_tx,
                                &lr.target,
                                &def.options
                            );
                        }
                        "tcp/custom" => {
                            LacWorker::tcp_custom(
                                thread_id,
                                &thread_tx,
                                &lr.target,
                                def.options.clone()
                            );
                        }
                        _ => {
                            let msg = LacMessage::new_log(
                                thread_id,
                                format!(
                                    "\n[{}] Skipping unknown protocol: {}\n",
                                    "ERROR".red(), def.protocol
                                )
                            );
                            thread_tx.send(msg).unwrap();
                        }
                    }
                }

                // Last request for this target
                lr.next_target = true;
                target_n += 1;

                thread_tx.send(lr).unwrap();
            }

            future::ok(())
        }));

        // Worker shutdown message
        let mut lr = LacMessage::new(thread_id);
        lr.last_message = true;
        self.thread_tx.send(lr).unwrap();
    }

    fn http_s(
        thread_id: u16,
        thread_tx: &mpsc::Sender<LacMessage>,
        target: &Target,
        options: &Options
    ) {
        let https = match HttpsConnector::new(4) {
            Ok(https) => https,
            Err(err) => {
                let msg = LacMessage::new_log(
                    thread_id,
                    format!(
                        "[{}] TLS initialization failed. Error: {}",
                        "ERROR".red(),
                        err
                    )
                );
                thread_tx.send(msg).unwrap();
                return
            }
        };
        let client = Client::builder()
            .keep_alive_timeout(Duration::from_secs(1))
            .retry_canceled_requests(false)
            .build::<_, hyper::Body>(https);

        for protocol in ["https", "http"].iter() {
            for port in &options.ports {
                let mut target_req = target.clone();
                target_req.protocol = protocol.to_string();
                target_req.port = *port;

                let target_err = target_req.clone();
                let target_timeout = target_req.clone();
                let thread_tx_req = thread_tx.clone();
                let thread_tx_err = thread_tx.clone();
                let thread_tx_timeout = thread_tx.clone();
                let req_fut = client
                    .get(
                        format!(
                            "{}://{}:{}",
                            target_req.protocol,
                            target_req.domain,
                            target_req.port
                        )
                        .parse()
                        .unwrap()
                    )
                    .and_then(move |res| {
                        let (parts, body) = res.into_parts();
                        body.concat2().map(move |body_content| {
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
                                "[{}][{}][{}:{}] - Target not available. Error: {}",
                                "INFO".yellow(),
                                target_err.protocol.to_uppercase().blue(),
                                target_err.domain.cyan(),
                                target_err.port.to_string().cyan(),
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
                                "[{}][{}][{}:{}] - Timeout reached",
                                "INFO".yellow(),
                                target_timeout.protocol.to_uppercase().blue(),
                                target_timeout.domain.cyan(),
                                target_timeout.port.to_string().cyan()
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
        thread_tx: &mpsc::Sender<LacMessage>,
        target: &Target,
        options: Options
    ) {
        for port in options.ports {
            let host = target.ip.clone();
            let addr = match format!("{}:{}", host, port).parse::<SocketAddr>() {
                Ok(addr) => addr,
                Err(_err) => {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "\n[{}] - Invalid address: {}\n",
                            "ERROR".red(),
                            format!("{}:{}", host, port).cyan()
                        )
                    );
                    thread_tx.send(msg).unwrap();
                    continue;
                }
            };

            let tx_fut = thread_tx.clone();
            let host_fut = host.clone();
            let tx_fut_conn_err = thread_tx.clone();
            let host_fut_conn_err = host.clone();
            let tx_fut_write_err = thread_tx.clone();
            let host_fut_write_err = host.clone();
            let tx_fut_read_err = thread_tx.clone();
            let host_fut_read_err = host.clone();
            let tx_fut_err = thread_tx.clone();
            let host_fut_err = host.clone();
            let message = options.message.clone().unwrap();
            let req_fut = TcpStream::connect(&addr)
                .map_err(move |err| {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "[{}][{}:{}] - TCP stream connection error: {}",
                            "INFO".yellow(),
                            host_fut_conn_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    );
                    tx_fut_conn_err.send(msg).unwrap();
                    err
                })
                .and_then(|stream| io::write_all(stream, message))
                .map_err(move |err| {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "[{}][{}:{}] - TCP stream write error: {}",
                            "INFO".yellow(),
                            host_fut_write_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    );
                    tx_fut_write_err.send(msg).unwrap();
                    err
                })
                .and_then(|(stream, _message)| {
                    let reader = BufReader::new(stream);
                    io::read_until(reader, '\n' as u8, Vec::new())
                })
                .map_err(move |err| {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "[{}][{}:{}] - TCP stream read error: {}",
                            "INFO".yellow(),
                            host_fut_read_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    );
                    tx_fut_read_err.send(msg).unwrap();
                    err
                })
                .and_then(move |(_stream, bytes)| {
                    let mut lr = LacMessage::new(thread_id);
                    lr.target.ip = host_fut;
                    lr.target.port = port;
                    lr.target.protocol = "tcp/custom".to_string();
                    lr.target.response = String::from_utf8_lossy(&bytes).to_string();
                    tx_fut.send(lr).unwrap();
                    Ok(())
                })
                .map_err(move |err| {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "[{}][{}:{}] - TCP error: {}",
                            "INFO".yellow(),
                            host_fut_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    );
                    tx_fut_err.send(msg).unwrap();
                });

            let timeout_host = host.clone();
            let thread_tx_timeout = thread_tx.clone();
            let req_timeout = Timeout::new(req_fut, Duration::from_secs(5))
                .map_err(move |_err| {
                    let msg = LacMessage::new_log(
                        thread_id,
                        format!(
                            "[{}][{}][{}:{}] - Timeout reached",
                            "INFO".yellow(),
                            "tcp/custom".blue(),
                            timeout_host.cyan(),
                            port.to_string().cyan(),
                        )
                    );
                    thread_tx_timeout.send(msg).unwrap();
                });
            rt::spawn(req_timeout);
        }
    }
}
