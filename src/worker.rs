use std::{
    collections::HashSet,
    fs::File,
    path::Path,
    sync::{mpsc::Sender, Arc, Mutex},
    time::Instant,
};

use easy_reader::EasyReader;
use hyper::{client::HttpConnector, Client};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{runtime, sync::Semaphore};
use tokio_tls::TlsConnector;

use crate::{
    conf::Conf,
    net::{self, HttpsRequest, TcpRequest},
};

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
                                net::tcp_custom(req).await;
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
                            net::http_s(req).await;
                            ws.release_permit();
                        });
                    }
                }
            }

            ws.targets += 1;
            tx.send(WorkerMessage::NextTarget).unwrap();
        }

        // Wait for existing connections to complete
        loop {
            let reqs = ws.requests.lock().unwrap();
            if reqs.completed == reqs.spawned {
                break;
            }
        }

        tx.send(WorkerMessage::Shutdown).unwrap();
    });
}
