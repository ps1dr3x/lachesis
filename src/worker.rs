use std::{
    collections::HashSet,
    fs::File,
    path::Path,
    sync::{mpsc::Sender, Arc, Mutex},
    time::Instant,
};

use easy_reader::EasyReader;
use futures::{stream::FuturesUnordered, StreamExt};
use hyper::{client::HttpConnector, Client};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{runtime, sync::Semaphore};
use tokio_tls::TlsConnector;

use crate::{
    conf::{Conf, Definition},
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

async fn check_ports(
    tx: Sender<WorkerMessage>,
    ws: WorkerState,
    defs: &[Definition],
    ip: String,
) -> HashSet<u16> {
    let mut unique_ports = HashSet::new();

    for def in defs {
        for port in &def.options.ports {
            unique_ports.insert(*port);
        }
    }

    let open_ports = Arc::new(Mutex::new(unique_ports.clone()));
    let mut futs = FuturesUnordered::new();
    for port in unique_ports {
        let tx = tx.clone();
        let ws = ws.clone();
        let ip = ip.clone();
        let open_ports = open_ports.clone();
        futs.push(async move {
            ws.wait_for_permit().await;

            let now = Instant::now();
            let timeout = ws.probe_time.lock().unwrap().timeout;
            let port_status = net::test_port(ip, port, timeout as u64).await;
            if !port_status.open {
                open_ports.lock().unwrap().remove(&port);
            }
            tx.send(WorkerMessage::PortStatus(port_status)).unwrap();
            // Timeout estimation formula from nmap
            // nmap.org/book/port-scanning-algorithms.html
            let rtt = now.elapsed().as_millis() as f32;
            let mut pt = ws.probe_time.lock().unwrap();
            let newsrtt = pt.srtt + (rtt - pt.srtt) / 8.0;
            let newrttvar = pt.rttvar + (f32::abs(rtt - pt.srtt) - pt.rttvar) / 4.0;
            pt.timeout = newsrtt + newrttvar * 4.0;

            ws.release_permit();
        });
    }

    loop {
        if futs.next().await.is_none() {
            break;
        }
    }

    Arc::try_unwrap(open_ports).unwrap().into_inner().unwrap()
}

fn build_https_client() -> Client<HttpsConnector<HttpConnector>> {
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
    Client::builder()
        //.pool_idle_timeout(Duration::from_millis(1250))
        //.http2_keep_alive_timeout(Duration::from_millis(1000))
        //.retry_canceled_requests(false)
        .build(https)
}

#[derive(Debug, Clone)]
pub struct PortStatus {
    pub ip: String,
    pub port: u16,
    pub open: bool,
    pub time: Instant,
    pub timeout: bool,
}

#[derive(Debug, Clone)]
pub enum WorkerMessage {
    PortStatus(PortStatus),
    Response(Target),
    Error(String, String),
    Timeout(String, String),
    NextTarget,
    Shutdown,
}

#[derive(Debug, Clone)]
struct WorkerProbeTime {
    srtt: f32,
    rttvar: f32,
    timeout: f32,
}

#[derive(Debug, Clone)]
struct WorkerRequests {
    spawned: u64,
    completed: u64,
}

#[derive(Debug, Clone)]
struct WorkerState {
    conf: Conf,
    https_client: Client<HttpsConnector<HttpConnector>>,
    semaphore: Arc<Semaphore>,
    requests: Arc<Mutex<WorkerRequests>>,
    probe_time: Arc<Mutex<WorkerProbeTime>>,
}

impl WorkerState {
    fn new(conf: Conf, https_client: Client<HttpsConnector<HttpConnector>>) -> Self {
        Self {
            conf,
            https_client,
            semaphore: Arc::new(Semaphore::new(500)),
            requests: Arc::new(Mutex::new(WorkerRequests {
                spawned: 0,
                completed: 0,
            })),
            probe_time: Arc::new(Mutex::new(WorkerProbeTime {
                srtt: 0.0,
                rttvar: 0.0,
                timeout: 3000.0,
            })),
        }
    }

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
        let ws = WorkerState::new(conf, build_https_client());

        let mut targets = 0;
        while ws.conf.max_targets == 0 || targets < ws.conf.max_targets {
            let target = if let Some(target) = get_next_target(&ws.conf) {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            let ws_in = ws.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let tx_in = tx.clone();
                let open_ports = check_ports(
                    tx_in,
                    ws_in.clone(),
                    &ws_in.conf.definitions,
                    target.ip.clone(),
                )
                .await;

                let mut http_s_ports = HashSet::new();
                for def in &ws_in.conf.definitions {
                    match def.protocol.as_str() {
                        "http/s" => {
                            // Only one http/s request per port
                            for port in &def.options.ports {
                                if open_ports.contains(port) {
                                    http_s_ports.insert(*port);
                                }
                            }
                        }
                        "tcp/custom" => {
                            for port in &def.options.ports {
                                if !open_ports.contains(port) {
                                    continue;
                                }

                                ws_in.wait_for_permit().await;

                                let mut target = target.clone();
                                target.domain = String::new();
                                target.protocol = "tcp/custom".to_string();
                                target.port = *port;
                                target.time = Instant::now();

                                let req = TcpRequest {
                                    tx: tx.clone(),
                                    target: target.clone(),
                                    message: def.options.message.clone().unwrap(),
                                    timeout: ws_in.conf.req_timeout,
                                };
                                net::tcp_custom(req).await;

                                ws_in.release_permit();
                            }
                        }
                        // Protocol field is already validated when conf is loaded
                        _ => (),
                    }
                }
                if !http_s_ports.is_empty() {
                    for protocol in ["https", "http"].iter() {
                        for port in &http_s_ports {
                            ws_in.wait_for_permit().await;

                            let mut target = target.clone();
                            target.protocol = protocol.to_string();
                            target.port = *port;
                            target.time = Instant::now();

                            let req = HttpsRequest {
                                tx: tx.clone(),
                                client: ws_in.https_client.clone(),
                                target: target.clone(),
                                user_agent: ws_in.conf.user_agent.clone(),
                                timeout: ws_in.conf.req_timeout,
                            };
                            net::http_s(req).await;

                            ws_in.release_permit();
                        }
                    }
                }
                tx.send(WorkerMessage::NextTarget).unwrap();
            });

            targets += 1;
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
