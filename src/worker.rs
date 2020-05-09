use std::{
    collections::HashSet,
    fs::File,
    net::SocketAddr,
    path::Path,
    sync::{mpsc::Sender, Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Buf;
use colored::Colorize;
use easy_reader::EasyReader;
use hyper::{client::HttpConnector, Body, Client, Request, Uri};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    runtime,
    sync::Semaphore,
    time::timeout,
};
use tokio_tls::TlsConnector;

use crate::conf::Conf;

use WorkerMessage::{Error, NextTarget, Response, Shutdown, Timeout};

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
    pub time: Instant,
}

impl Target {
    pub fn default() -> Target {
        Target {
            domain: String::new(),
            ip: String::new(),
            port: 0,
            protocol: String::new(),
            response: String::new(),
            time: Instant::now(),
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
    Error(String, String),
    Timeout(String, String),
    NextTarget,
    Shutdown,
}

struct HttpsRequest {
    tx: Sender<WorkerMessage>,
    client: Client<HttpsConnector<HttpConnector>>,
    target: Target,
    protocol: String,
    port: u16,
    user_agent: String,
    timeout: u64,
}

async fn http_s(req: HttpsRequest) {
    let mut target = req.target.clone();
    target.protocol = req.protocol;
    target.port = req.port;
    target.time = Instant::now();

    let uri: Uri = format!("{}://{}:{}", target.protocol, target.ip, target.port)
        .parse()
        .unwrap();

    let request = Request::builder()
        .uri(uri)
        .header("Host", target.domain.clone())
        .header("User-Agent", req.user_agent.clone())
        .header("Accept", "*/*")
        .body(Body::empty())
        .unwrap();

    let request = match timeout(
        Duration::from_secs(req.timeout / 2),
        req.client.request(request),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => {
            req.tx
                .send(Timeout(
                    format!(
                        "[{}][{}:{}] - Request timeout",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let (parts, body) = match request {
        Ok(r) => r.into_parts(),
        Err(e) => {
            req.tx
                .send(Error(
                    format!(
                        "[{}][{}:{}] - Request error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let body = match timeout(
        Duration::from_secs(req.timeout / 2),
        hyper::body::aggregate(body),
    )
    .await
    {
        Ok(a) => a,
        Err(_) => {
            req.tx
                .send(Timeout(
                    format!(
                        "[{}][{}:{}] - Response body timeout",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    match body {
        Ok(b) => {
            // Merge response's headers and body
            let mut raw_content = format!("{:?} {}\r\n", parts.version, parts.status);
            for header in &parts.headers {
                raw_content = format!(
                    "{}{}: {}\r\n",
                    raw_content,
                    header.0,
                    header.1.to_str().unwrap_or("")
                );
            }
            raw_content = format!("{}\r\n{}", raw_content, String::from_utf8_lossy(b.bytes()));
            target.response = raw_content;

            req.tx.send(Response(target)).unwrap();
        }
        Err(e) => {
            req.tx
                .send(Error(
                    format!(
                        "[{}][{}:{}] - Response error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.domain.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
        }
    };
}

struct TcpRequest {
    tx: Sender<WorkerMessage>,
    target: Target,
    port: u16,
    message: String,
    timeout: u64,
}

async fn tcp_custom(req: TcpRequest) {
    let mut target = req.target.clone();
    target.domain = String::new();
    target.protocol = "tcp/custom".to_string();
    target.port = req.port;
    target.time = Instant::now();

    let addr = match format!("{}:{}", target.ip, target.port).parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_e) => {
            req.tx
                .send(Error(
                    format!(
                        "[{}] Invalid address: {}:{}",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        req.port.to_string().cyan()
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let stream = match timeout(
        Duration::from_secs(req.timeout / 3),
        TcpStream::connect(&addr),
    )
    .await
    {
        Ok(s) => s,
        Err(_) => {
            req.tx
                .send(Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp connection timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    let mut stream = match stream {
        Ok(s) => s,
        Err(e) => {
            req.tx
                .send(Error(
                    format!(
                        "[{}][{}:{}] - TCP stream connection error: {}",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                        e
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    match timeout(
        Duration::from_secs(req.timeout / 3),
        stream.write_all(req.message.as_bytes()),
    )
    .await
    {
        Ok(w) => {
            if let Err(e) = w {
                req.tx
                    .send(Error(
                        format!(
                            "[{}][{}:{}] - TCP stream write error: {}",
                            target.protocol.to_uppercase().blue(),
                            target.ip.cyan(),
                            target.port.to_string().cyan(),
                            e
                        ),
                        target.protocol,
                    ))
                    .unwrap();
                return;
            }
        }
        Err(_) => {
            req.tx
                .send(Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp stream write timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    // FIXME - find a better way to read the answer
    let mut answer = [0; 100_000];
    match timeout(
        Duration::from_secs(req.timeout / 3),
        stream.read(&mut answer),
    )
    .await
    {
        Ok(w) => {
            if let Err(e) = w {
                req.tx
                    .send(Error(
                        format!(
                            "[{}][{}:{}] - TCP stream read error: {}",
                            target.protocol.to_uppercase().blue(),
                            target.ip.cyan(),
                            target.port.to_string().cyan(),
                            e
                        ),
                        target.protocol,
                    ))
                    .unwrap();
                return;
            }
        }
        Err(_) => {
            req.tx
                .send(Timeout(
                    format!(
                        "[{}][{}:{}] - Tcp stream read timeout",
                        target.protocol.to_uppercase().blue(),
                        target.ip.cyan(),
                        target.port.to_string().cyan(),
                    ),
                    target.protocol,
                ))
                .unwrap();
            return;
        }
    };

    target.response = String::from_utf8_lossy(&answer).to_string();
    req.tx.send(Response(target)).unwrap();
}

fn get_next_target(conf: &Conf) -> Option<Target> {
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

#[derive(Debug, Clone)]
struct WorkerRequests {
    spawned: u64,
    completed: u64,
    successful: u64,
    avg_time: u128,
}

#[derive(Debug, Clone)]
struct WorkerState {
    semaphore: Arc<Semaphore>,
    targets: u64,
    requests: Arc<Mutex<WorkerRequests>>,
}

impl WorkerState {
    async fn wait_for_permit(&self) {
        self.semaphore.acquire().await.forget();
        self.requests.lock().unwrap().spawned += 1;
    }

    fn release_permit(&self) {
        self.semaphore.add_permits(1);
        self.requests.lock().unwrap().completed += 1;
    }
}

pub fn run(tx: Sender<WorkerMessage>, conf: Conf) {
    let mut rt = runtime::Builder::new()
        .threaded_scheduler()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let mut ws = WorkerState {
            semaphore: Arc::new(Semaphore::new(500)),
            targets: 0,
            requests: Arc::new(Mutex::new(WorkerRequests {
                spawned: 0,
                completed: 0,
                successful: 0,
                avg_time: 0,
            })),
        };

        // TODOs:
        // - Tweak connectors and client configuration
        // - Try using rustls instead of native_tls as TLS connector
        let mut http = HttpConnector::new();
        //http.set_connect_timeout(Some(Duration::from_millis(1000)));
        http.enforce_http(false);
        let tls_connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        let tls_connector = TlsConnector::from(tls_connector);
        let https = HttpsConnector::from((http, tls_connector));
        let https_client = Client::builder()
            //.pool_idle_timeout(Duration::from_millis(1250))
            //.http2_keep_alive_timeout(Duration::from_millis(1000))
            //.retry_canceled_requests(false)
            .build(https);

        while conf.max_targets == 0 || ws.targets < conf.max_targets {
            let target = if let Some(target) = get_next_target(&conf) {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            let mut http_s_ports = HashSet::new();
            for def in &conf.definitions {
                match def.protocol.as_str() {
                    "http/s" => {
                        // Only one http/s request per port
                        for port in &def.options.ports {
                            http_s_ports.insert(*port);
                        }
                    }
                    "tcp/custom" => {
                        for port in &def.options.ports {
                            ws.wait_for_permit().await;
                            let ws = ws.clone();
                            let req = TcpRequest {
                                tx: tx.clone(),
                                target: target.clone(),
                                port: *port,
                                message: def.options.message.clone().unwrap(),
                                timeout: conf.req_timeout,
                            };
                            tokio::spawn(async move {
                                tcp_custom(req).await;
                                ws.release_permit();
                            });
                        }
                    }
                    _ => (),
                }
            }
            if !http_s_ports.is_empty() {
                for protocol in ["https", "http"].iter() {
                    for port in &http_s_ports {
                        ws.wait_for_permit().await;
                        let ws = ws.clone();
                        let req = HttpsRequest {
                            tx: tx.clone(),
                            client: https_client.clone(),
                            target: target.clone(),
                            protocol: protocol.to_string(),
                            port: *port,
                            user_agent: conf.user_agent.clone(),
                            timeout: conf.req_timeout,
                        };
                        tokio::spawn(async move {
                            http_s(req).await;
                            ws.release_permit();
                        });
                    }
                }
            }

            ws.targets += 1;
            tx.send(NextTarget).unwrap();
        }

        // Wait for existing connections to complete
        loop {
            let reqs = ws.requests.lock().unwrap();
            if reqs.completed == reqs.spawned {
                break;
            }
        }

        tx.send(Shutdown).unwrap();
    });
}
