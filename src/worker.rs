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
            domain: String::new(),
            ip: String::new(),
            port: 0,
            protocol: String::new(),
            response: String::new()
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
pub struct WorkerMessage {
    pub message: String,
    next_target: bool,
    pub target: Target,
    last_message: bool
}

impl WorkerMessage {
    fn default() -> WorkerMessage {
        WorkerMessage {
            message: String::new(),
            next_target: false,
            target: Target::default(),
            last_message: false
        }
    }

    fn log(message: String) -> Self {
        WorkerMessage {
            message,
            ..WorkerMessage::default()
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

pub fn run(tx: &mpsc::Sender<WorkerMessage>, conf: LacConf) {
    let tx_inner = tx.clone();
    rt::run(lazy(move || {
        let mut target_n = 0;
        while conf.max_targets == 0 || target_n < conf.max_targets {
            let mut lr = WorkerMessage::default();
            let target = if !conf.dataset.is_empty() {
                // If dataset mode, open and instantiate the reader
                let dataset_path = Path::new(conf.dataset.as_str());
                let dataset_file = File::open(dataset_path).unwrap();
                let mut easy_reader = EasyReader::new(dataset_file).unwrap();

                // Pick a random dns record (excluding records which are not of type A)
                let line_str = easy_reader.random_line().unwrap().unwrap();
                let dataset_record: DatasetRecord = serde_json::from_str(&line_str).unwrap();
                if dataset_record.record_type != "a" { continue; }

                Some(Target::new(dataset_record.name, dataset_record.value))
            } else {
                // If subnet mode, pick the next ip in the specified subnets
                let mut current_subnet_idx = conf.subnets.lock().unwrap().1;
                let mut ip = conf.subnets.lock().unwrap().0[current_subnet_idx].next();
                while ip.is_none() {
                    conf.subnets.lock().unwrap().1 += 1;
                    current_subnet_idx = conf.subnets.lock().unwrap().1;
                    if current_subnet_idx >= conf.subnets.lock().unwrap().0.len() {
                        break;
                    } else {
                        ip = conf.subnets.lock().unwrap().0[current_subnet_idx].next();
                    }
                }
                match ip {
                    Some(ip) => {
                        let ip_s = ip.to_string();
                        Some(Target::new(ip_s.clone(), ip_s))
                    }
                    None => None
                }
            };

            if let Some(target) = target {
                lr.target = target;
            } else {
                // All the targets have been consumed
                break;
            }

            // Requests
            for def in &conf.definitions {
                match def.protocol.as_str() {
                    "http/s" => {
                        http_s(
                            &tx_inner,
                            &lr.target,
                            &def.options
                        );
                    }
                    "tcp/custom" => {
                        tcp_custom(
                            &tx_inner,
                            &lr.target,
                            def.options.clone()
                        );
                    }
                    _ => {
                        let msg = WorkerMessage::log(
                            format!(
                                "\n[{}] Skipping unknown protocol: {}\n",
                                "ERROR".red(), def.protocol
                            )
                        );
                        tx_inner.send(msg).unwrap();
                    }
                }
            }

            // Last request for this target
            lr.next_target = true;
            target_n += 1;

            tx_inner.send(lr).unwrap();
        }

        future::ok(())
    }));

    // Worker shutdown message
    let mut lr = WorkerMessage::default();
    lr.last_message = true;
    tx.send(lr).unwrap();
}

fn http_s(
    thread_tx: &mpsc::Sender<WorkerMessage>,
    target: &Target,
    options: &Options
) {
    let https = match HttpsConnector::new(4) {
        Ok(https) => https,
        Err(err) => {
            let msg = WorkerMessage::log(
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
                        let mut lr = WorkerMessage::default();
                        lr.target = target_req;
                        thread_tx_req.send(lr).unwrap();
                    })
                })
                .map_err(move |err| {
                    let msg = WorkerMessage::log(
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
                    let msg = WorkerMessage::log(
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
    thread_tx: &mpsc::Sender<WorkerMessage>,
    target: &Target,
    options: Options
) {
    for port in options.ports {
        let host = target.ip.clone();
        let addr = match format!("{}:{}", host, port).parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(_err) => {
                let msg = WorkerMessage::log(
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
                let msg = WorkerMessage::log(
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
                let msg = WorkerMessage::log(
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
                io::read_until(reader, b'\n', Vec::new())
            })
            .map_err(move |err| {
                let msg = WorkerMessage::log(
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
                let mut lr = WorkerMessage::default();
                lr.target.ip = host_fut;
                lr.target.port = port;
                lr.target.protocol = "tcp/custom".to_string();
                lr.target.response = String::from_utf8_lossy(&bytes).to_string();
                tx_fut.send(lr).unwrap();
                Ok(())
            })
            .map_err(move |err| {
                let msg = WorkerMessage::log(
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
                let msg = WorkerMessage::log(
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
