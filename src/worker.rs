use std::{collections::HashSet, fs::File, path::Path, sync::Arc, time::Instant};

use easy_reader::EasyReader;
use hyper::client::{Client, HttpConnector};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    runtime::Builder,
    sync::{mpsc::Sender, Mutex, Semaphore},
};

use crate::{
    conf::{Conf, Definition},
    net::{self, HttpsRequest, TcpRequest},
};

// Timeout estimation formula from nmap
// nmap.org/book/port-scanning-algorithms.html
fn estimate_timeout(oldsrtt: f32, curr_rtt: f32, oldrttvar: f32) -> f32 {
    let newsrtt = oldsrtt + (curr_rtt - oldsrtt) / 8.0;
    let newrttvar = oldrttvar + (f32::abs(curr_rtt - oldsrtt) - oldrttvar) / 4.0;
    newsrtt + newrttvar * 4.0
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
    let mut handlers = Vec::new();
    for port in unique_ports {
        let tx = tx.clone();
        let ws = ws.clone();
        let ip = ip.clone();
        let open_ports = open_ports.clone();
        let handler = tokio::spawn(async move {
            ws.wait_for_permit().await;

            let now = Instant::now();
            let timeout = ws.probe_time.lock().await.timeout;
            let port_target = net::test_port(ip, port, timeout as u64).await;
            if port_target.status != PortStatus::Open {
                open_ports.lock().await.remove(&port);
            }
            tx.send(WorkerMessage::PortTarget(port_target))
                .await
                .unwrap();

            let rtt = now.elapsed().as_millis() as f32;
            let mut pt = ws.probe_time.lock().await;
            pt.timeout = estimate_timeout(pt.srtt, rtt, pt.rttvar);

            ws.release_permit().await;
        });

        handlers.push(handler);
    }

    for handler in handlers {
        handler.await.unwrap();
    }

    Arc::try_unwrap(open_ports).unwrap().into_inner()
}

#[derive(Debug, Clone)]
pub struct ReqTarget {
    pub domain: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub response: String,
    pub time: Instant,
}

impl ReqTarget {
    pub fn default() -> ReqTarget {
        ReqTarget {
            domain: String::new(),
            ip: String::new(),
            port: 0,
            protocol: String::new(),
            response: String::new(),
            time: Instant::now(),
        }
    }

    fn new(domain: String, ip: String) -> Self {
        ReqTarget {
            domain,
            ip,
            ..ReqTarget::default()
        }
    }
}

async fn target_requests(tx: Sender<WorkerMessage>, ws: WorkerState, target: ReqTarget) {
    let tx_in = tx.clone();
    let open_ports = check_ports(tx_in, ws.clone(), &ws.conf.definitions, target.ip.clone()).await;

    let mut http_s_ports = HashSet::new();
    for def in &ws.conf.definitions {
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

                    ws.wait_for_permit().await;

                    let mut target = target.clone();
                    target.domain = String::new();
                    target.protocol = "tcp/custom".to_string();
                    target.port = *port;
                    target.time = Instant::now();

                    let req = TcpRequest {
                        tx: tx.clone(),
                        target: target.clone(),
                        message: def.options.message.clone().unwrap(),
                        timeout: ws.conf.req_timeout,
                    };
                    net::tcp_custom(req).await;

                    ws.release_permit().await;
                }
            }
            // Protocol field is already validated when conf is loaded
            _ => (),
        }
    }
    if !http_s_ports.is_empty() {
        for protocol in ["https", "http"].iter() {
            for port in &http_s_ports {
                if (*port == 80 && *protocol == "https") || (*port == 443 && *protocol == "http") {
                    continue;
                }

                ws.wait_for_permit().await;

                let mut target = target.clone();
                target.protocol = protocol.to_string();
                target.port = *port;
                target.time = Instant::now();

                let req = HttpsRequest {
                    tx: tx.clone(),
                    client: ws.https_client.clone(),
                    target: target.clone(),
                    user_agent: ws.conf.user_agent.clone(),
                    timeout: ws.conf.req_timeout,
                };
                net::http_s(req).await;

                ws.release_permit().await;
            }
        }
    }

    tx.send(WorkerMessage::NextTarget).await.unwrap();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String,
}

fn get_next_target(conf: &Conf) -> Option<ReqTarget> {
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
            return Some(ReqTarget::new(dataset_record.name, dataset_record.value));
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

        ip.map(|ip| ReqTarget::new(ip.to_string(), ip.to_string()))
    }
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
    targets_count: u64,
    semaphore: Arc<Semaphore>,
    requests: Arc<Mutex<WorkerRequests>>,
    probe_time: Arc<Mutex<WorkerProbeTime>>,
}

impl WorkerState {
    fn new(conf: Conf, https_client: Client<HttpsConnector<HttpConnector>>) -> Self {
        let max_concurrent_requests = conf.max_concurrent_requests as usize;
        Self {
            conf,
            https_client,
            targets_count: 0,
            semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
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
        self.semaphore.acquire().await.unwrap().forget();
        self.requests.lock().await.spawned += 1;
    }

    async fn release_permit(&self) {
        self.semaphore.add_permits(1);
        self.requests.lock().await.completed += 1;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortStatus {
    Open,
    Closed,
    Timedout,
}

#[derive(Debug, Clone)]
pub struct PortTarget {
    pub ip: String,
    pub port: u16,
    pub status: PortStatus,
    pub time: Instant,
}

#[derive(Debug, Clone)]
pub enum WorkerMessage {
    PortTarget(PortTarget),
    Response(ReqTarget),
    Fail(ReqTarget, String, Option<String>),
    Timeout(ReqTarget),
    NextTarget,
    Shutdown,
}

pub fn run(tx: Sender<WorkerMessage>, conf: Conf) {
    let rt = Builder::new_multi_thread()
        .worker_threads(num_cpus::get())
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let mut ws = WorkerState::new(conf, net::build_https_client());

        while ws.conf.max_targets == 0 || ws.targets_count < ws.conf.max_targets {
            let target = if let Some(target) = get_next_target(&ws.conf) {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            let ws_in = ws.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                target_requests(tx, ws_in, target).await;
            });
            ws.targets_count += 1;
        }

        // Wait for existing connections to complete
        loop {
            let reqs = ws.requests.lock().await;
            if reqs.completed == reqs.spawned {
                break;
            }
        }

        tx.send(WorkerMessage::Shutdown).await.unwrap();
    });
}
