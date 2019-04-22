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
    Uri,
    Request,
    Body,
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
pub enum WorkerMessage {
    Response(Target),
    Log(String),
    NextTarget,
    Shutdown
}

pub fn run(tx: &mpsc::Sender<WorkerMessage>, conf: LacConf) {
    let tx_inner = tx.clone();
    rt::run(lazy(move || {
        let mut target_n = 0;
        while conf.max_targets == 0 || target_n < conf.max_targets {
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

            let target = if let Some(target) = target {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            // Requests
            for def in &conf.definitions {
                match def.protocol.as_str() {
                    "http/s" => {
                        http_s(
                            &tx_inner,
                            &target,
                            &def.options,
                            conf.user_agent.clone()
                        );
                    }
                    "tcp/custom" => {
                        tcp_custom(
                            &tx_inner,
                            &target,
                            def.options.clone()
                        );
                    }
                    _ => {
                        tx_inner.send(
                            WorkerMessage::Log(
                                format!(
                                    "\n[{}] Skipping unknown protocol: {}\n",
                                    "ERROR".red(), def.protocol
                                )
                            )
                        ).unwrap();
                    }
                }
            }

            // Increment the targets counts
            target_n += 1;
            tx_inner.send(WorkerMessage::NextTarget).unwrap();
        }

        future::ok(())
    }));

    // Send the worker shutdown message
    tx.send(WorkerMessage::Shutdown).unwrap();
}

fn http_s(
    thread_tx: &mpsc::Sender<WorkerMessage>,
    target: &Target,
    options: &Options,
    user_agent: String
) {
    let https = match HttpsConnector::new(4) {
        Ok(https) => https,
        Err(err) => {
            thread_tx.send(
                WorkerMessage::Log(
                    format!(
                        "[{}] TLS initialization failed. Error: {}",
                        "ERROR".red(),
                        err
                    )
                )
            ).unwrap();
            return
        }
    };
    let client = Client::builder()
        .keep_alive_timeout(Duration::from_secs(1))
        .retry_canceled_requests(false)
        .build::<_, hyper::Body>(https);

    for protocol in ["https", "http"].iter() {
        for port in &options.ports {
            let mut target = target.clone();
            target.protocol = protocol.to_string();
            target.port = *port;

            let target_err = target.clone();
            let target_timeout = target.clone();
            let thread_tx_req = thread_tx.clone();
            let thread_tx_err = thread_tx.clone();
            let thread_tx_timeout = thread_tx.clone();

            let uri: Uri = format!(
                    "{}://{}:{}",
                    target.protocol,
                    target.ip,
                    target.port
                )
                .parse()
                .unwrap();

            let request = Request::builder()
                .uri(uri)
                .header("Host", target.domain.clone())
                .header("User-Agent", user_agent.clone())
                .header("Accept", "*/*")
                .body(Body::empty())
                .unwrap();
            
            let req_fut = client
                .request(request)
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
                        target.response = raw_content;
                        // Send the response
                        thread_tx_req.send(WorkerMessage::Response(target)).unwrap();
                    })
                })
                .map_err(move |err| {
                    thread_tx_err.send(
                        WorkerMessage::Log(
                            format!(
                                "[{}][{}][{}:{}] - Target not available. Error: {}",
                                "INFO".yellow(),
                                target_err.protocol.to_uppercase().blue(),
                                target_err.domain.cyan(),
                                target_err.port.to_string().cyan(),
                                err
                            )
                        )
                    ).unwrap();
                });

            let req_timeout = Timeout::new(req_fut, Duration::from_secs(5))
                .map_err(move |_err| {
                    thread_tx_timeout.send(
                        WorkerMessage::Log(
                            format!(
                                "[{}][{}][{}:{}] - Timeout reached",
                                "INFO".yellow(),
                                target_timeout.protocol.to_uppercase().blue(),
                                target_timeout.domain.cyan(),
                                target_timeout.port.to_string().cyan()
                            )
                        )
                    ).unwrap();
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
                thread_tx.send(
                    WorkerMessage::Log(
                        format!(
                            "\n[{}] - Invalid address: {}\n",
                            "ERROR".red(),
                            format!("{}:{}", host, port).cyan()
                        )
                    )
                ).unwrap();
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
                tx_fut_conn_err.send(
                    WorkerMessage::Log(
                        format!(
                            "[{}][{}:{}] - TCP stream connection error: {}",
                            "INFO".yellow(),
                            host_fut_conn_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    )
                ).unwrap();
                err
            })
            .and_then(|stream| io::write_all(stream, message))
            .map_err(move |err| {
                tx_fut_write_err.send(
                    WorkerMessage::Log(
                        format!(
                            "[{}][{}:{}] - TCP stream write error: {}",
                            "INFO".yellow(),
                            host_fut_write_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    )
                ).unwrap();
                err
            })
            .and_then(|(stream, _message)| {
                let reader = BufReader::new(stream);
                io::read_until(reader, b'\n', Vec::new())
            })
            .map_err(move |err| {
                tx_fut_read_err.send(
                    WorkerMessage::Log(
                        format!(
                            "[{}][{}:{}] - TCP stream read error: {}",
                            "INFO".yellow(),
                            host_fut_read_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    )
                ).unwrap();
                err
            })
            .and_then(move |(_stream, bytes)| {
                let target = Target {
                    domain: String::new(),
                    ip: host_fut,
                    port,
                    protocol: "tcp/custom".to_string(),
                    response: String::from_utf8_lossy(&bytes).to_string()
                };
                tx_fut.send(WorkerMessage::Response(target)).unwrap();
                Ok(())
            })
            .map_err(move |err| {
                tx_fut_err.send(
                    WorkerMessage::Log(
                        format!(
                            "[{}][{}:{}] - TCP error: {}",
                            "INFO".yellow(),
                            host_fut_err.cyan(),
                            port.to_string().cyan(),
                            err
                        )
                    )
                ).unwrap();
            });

        let timeout_host = host.clone();
        let thread_tx_timeout = thread_tx.clone();
        let req_timeout = Timeout::new(req_fut, Duration::from_secs(5))
            .map_err(move |_err| {
                thread_tx_timeout.send(
                    WorkerMessage::Log(
                        format!(
                            "[{}][{}][{}:{}] - Timeout reached",
                            "INFO".yellow(),
                            "tcp/custom".blue(),
                            timeout_host.cyan(),
                            port.to_string().cyan(),
                        )
                    )
                ).unwrap();
            });
        rt::spawn(req_timeout);
    }
}
