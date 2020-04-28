use bytes::Buf;
use colored::Colorize;
use easy_reader::EasyReader;
use hyper::{client::HttpConnector, Body, Client, Request, Uri};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Semaphore,
};
use tokio_tls::TlsConnector;

use std::{
    collections::HashSet,
    fs::File,
    net::SocketAddr,
    path::Path,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use crate::lachesis::{LacConf, Options};
use WorkerMessage::{LogErr, LogInfo, NextTarget, Response, Shutdown};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub domain: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub response: String,
}

impl Target {
    pub fn default() -> Target {
        Target {
            domain: String::new(),
            ip: String::new(),
            port: 0,
            protocol: String::new(),
            response: String::new(),
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
    LogInfo(String),
    LogErr(String),
    NextTarget,
    Shutdown,
}

async fn http_s(
    tx: mpsc::Sender<WorkerMessage>,
    client: Client<HttpsConnector<HttpConnector>>,
    target: Target,
    ports: HashSet<u16>,
    user_agent: String,
) {
    for protocol in ["https", "http"].iter() {
        for port in &ports {
            let mut target = target.clone();
            target.protocol = (*protocol).to_string();
            target.port = *port;

            let uri: Uri = format!("{}://{}:{}", target.protocol, target.ip, target.port)
                .parse()
                .unwrap();

            let request = Request::builder()
                .uri(uri)
                .header("Host", target.domain.clone())
                .header("User-Agent", user_agent.clone())
                .header("Accept", "*/*")
                .body(Body::empty())
                .unwrap();

            match client.request(request).await {
                Ok(r) => {
                    let (parts, body) = r.into_parts();
                    match hyper::body::aggregate(body).await {
                        Ok(b) => {
                            // Merge response's headers and body
                            let mut raw_content =
                                format!("{:?} {}\r\n", parts.version, parts.status);
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
                                String::from_utf8_lossy(b.bytes())
                            );
                            target.response = raw_content;
                            // Send the response
                            tx.send(Response(target)).unwrap();
                        }
                        Err(e) => {
                            tx.send(LogInfo(format!(
                                "[{}][{}:{}] - Target not available. Error: {}",
                                target.protocol.to_uppercase().blue(),
                                target.domain.cyan(),
                                target.port.to_string().cyan(),
                                e
                            )))
                            .unwrap();
                        }
                    }
                }
                Err(e) => {
                    tx.send(LogInfo(format!(
                        "[{}][{}:{}] - Target not available. Error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                        e
                    )))
                    .unwrap();
                }
            };
        }
    }
}

async fn tcp_custom(tx: mpsc::Sender<WorkerMessage>, target: Target, options: Options) {
    for port in &options.ports {
        let mut target = target.clone();
        target.domain = String::new();
        target.protocol = "tcp/custom".to_string();
        target.port = *port;

        let addr = match format!("{}:{}", target.ip, target.port).parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(_e) => {
                tx.send(LogErr(format!(
                    "[{}] Invalid address: {}:{}",
                    target.protocol.to_uppercase().blue(),
                    target.ip.cyan(),
                    port.to_string().cyan()
                )))
                .unwrap();
                continue;
            }
        };

        let mut stream = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                tx.send(LogInfo(format!(
                    "[{}][{}:{}] - TCP stream connection error: {}",
                    target.protocol.to_uppercase().blue(),
                    target.ip.cyan(),
                    target.port.to_string().cyan(),
                    e
                )))
                .unwrap();
                continue;
            }
        };

        let message = options.message.clone().unwrap();
        match stream.write_all(message.as_bytes()).await {
            Ok(_) => (),
            Err(e) => {
                tx.send(LogInfo(format!(
                    "[{}][{}:{}] - TCP stream write error: {}",
                    target.protocol.to_uppercase().blue(),
                    target.ip.cyan(),
                    target.port.to_string().cyan(),
                    e
                )))
                .unwrap();
                continue;
            }
        };

        // FIXME - find a better way to read the answer
        let mut answer = [0; 100_000];
        match stream.read(&mut answer).await {
            Ok(_) => (),
            Err(e) => {
                tx.send(LogInfo(format!(
                    "[{}][{}:{}] - TCP stream read error: {}",
                    target.protocol.to_uppercase().blue(),
                    target.ip.cyan(),
                    target.port.to_string().cyan(),
                    e
                )))
                .unwrap();
                continue;
            }
        };

        if !answer.is_empty() {
            target.response = String::from_utf8_lossy(&answer).to_string();
            tx.send(Response(target)).unwrap();
        }
    }
}

fn get_next_target(conf: &LacConf) -> Option<Target> {
    if !conf.dataset.is_empty() {
        // If dataset mode, pick a random dns record
        // (excluding records which are not of type A)
        let dataset_path = Path::new(conf.dataset.as_str());
        let dataset_file = File::open(dataset_path).unwrap();
        let mut easy_reader = EasyReader::new(dataset_file).unwrap();
        loop {
            let line_str = easy_reader.random_line().unwrap().unwrap();
            let dataset_record: DatasetRecord = serde_json::from_str(&line_str).unwrap();
            if dataset_record.record_type != "a" {
                continue;
            }
            return Some(Target::new(dataset_record.name, dataset_record.value));
        }
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
            Some(ip) => Some(Target::new(ip.to_string(), ip.to_string())),
            None => None,
        }
    }
}

pub fn run(tx: mpsc::Sender<WorkerMessage>, conf: LacConf) {
    let mut rt = tokio::runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let mut http = HttpConnector::new();
        http.set_connect_timeout(Some(Duration::from_millis(2500)));
        http.set_happy_eyeballs_timeout(Some(Duration::from_secs(1)));
        http.enforce_http(false);
        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        let connector = TlsConnector::from(connector);
        let https = HttpsConnector::from((http, connector));
        let client = Client::builder()
            .pool_idle_timeout(Duration::from_millis(2500))
            .retry_canceled_requests(false)
            .build(https);

        let semaphore = Arc::new(Semaphore::new(500));
        let mut targets = 0;
        let completed = Arc::new(Mutex::new(0));
        while conf.max_targets == 0 || targets < conf.max_targets {
            let target = if let Some(target) = get_next_target(&conf) {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            let mut http_s_ports = HashSet::new();
            for def in conf.clone().definitions {
                match def.protocol.as_str() {
                    "http/s" => {
                        // Only one http/s request per port
                        for port in def.options.ports {
                            http_s_ports.insert(port);
                        }
                    }
                    "tcp/custom" => {
                        let tx = tx.clone();
                        let target = target.clone();
                        let options = def.options.clone();
                        let completed = completed.clone();
                        let semaphore = semaphore.clone();
                        semaphore.acquire().await.forget();
                        tokio::spawn(async move {
                            tcp_custom(tx, target, options).await;
                            *completed.lock().unwrap() += 1;
                            semaphore.add_permits(1);
                        });
                    }
                    _ => (),
                }
            }
            if !http_s_ports.is_empty() {
                let tx = tx.clone();
                let client = client.clone();
                let uagent = conf.user_agent.clone();
                let completed = completed.clone();
                let semaphore = semaphore.clone();
                semaphore.acquire().await.forget();
                tokio::spawn(async move {
                    http_s(tx, client, target, http_s_ports, uagent).await;
                    *completed.lock().unwrap() += 1;
                    semaphore.add_permits(1);
                });
            }

            targets += 1;
            tx.send(NextTarget).unwrap();
        }

        while *completed.lock().unwrap() < conf.max_targets {
            // Wait for existing connections to complete
        }

        tx.send(Shutdown).unwrap();
    });
}
