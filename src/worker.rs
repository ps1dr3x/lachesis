use std::{
    collections::HashSet,
    fs::File,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use easy_reader::EasyReader;
use hyper::client::{Client, HttpConnector};
use hyper_tls::HttpsConnector;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc::Sender, Mutex, Semaphore},
    time::{sleep, Duration},
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

    let mut open_ports = unique_ports.clone();
    for port in unique_ports {
        ws.maybe_wait_for_permit().await;

        let now = Instant::now();
        let timeout = ws.probe_time.lock().await.timeout;
        let port_target = net::test_port(ip.clone(), port, timeout as u64).await;

        if port_target.status != PortStatus::Open {
            open_ports.remove(&port);
        }

        tx.send(WorkerMessage::PortTarget(port_target))
            .await
            .unwrap();

        let rtt = now.elapsed().as_millis() as f32;
        let mut pt = ws.probe_time.lock().await;
        pt.timeout = estimate_timeout(pt.srtt, rtt, pt.rttvar);

        ws.maybe_release_permit().await;
    }

    open_ports
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
    let open_ports = check_ports(
        tx.clone(),
        ws.clone(),
        &ws.conf.definitions,
        target.ip.clone(),
    )
    .await;

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

                    ws.maybe_wait_for_permit().await;

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

                    ws.maybe_release_permit().await;
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

                ws.maybe_wait_for_permit().await;

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

                ws.maybe_release_permit().await;
            }
        }
    }

    ws.targets_completed.fetch_add(1, Ordering::SeqCst);
    tx.send(WorkerMessage::NextTarget).await.unwrap();
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatasetRecord {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub value: String,
}

// Pick a random dns record from the dataset
// (excluding records which are not of type A)
async fn get_next_dataset_target(dataset: &mut EasyReader<File>) -> Option<ReqTarget> {
    loop {
        let line_str = dataset.random_line().unwrap().unwrap();
        let dataset_record: DatasetRecord = serde_json::from_str(&line_str).unwrap();
        if dataset_record.record_type != "a" {
            continue;
        }
        return Some(ReqTarget::new(dataset_record.name, dataset_record.value));
    }
}

// Pick the next ip in the specified subnets
async fn get_next_subnet_target(conf: &Conf) -> Option<ReqTarget> {
    let mut current_subnet_idx = conf.subnets.lock().await.1;
    let mut ip = conf.subnets.lock().await.0[current_subnet_idx].next();

    while ip.is_none() {
        conf.subnets.lock().await.1 += 1;
        current_subnet_idx = conf.subnets.lock().await.1;
        if current_subnet_idx >= conf.subnets.lock().await.0.len() {
            break;
        } else {
            ip = conf.subnets.lock().await.0[current_subnet_idx].next();
        }
    }

    ip.map(|ip| ReqTarget::new(ip.to_string(), ip.to_string()))
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
    targets_completed: Arc<AtomicU64>,
    semaphore: Arc<Semaphore>,
    probe_time: Arc<Mutex<WorkerProbeTime>>,
}

impl WorkerState {
    fn new(conf: Conf, https_client: Client<HttpsConnector<HttpConnector>>) -> Self {
        let max_concurrent_requests = conf.max_concurrent_requests;

        Self {
            conf,
            https_client,
            targets_count: 0,
            targets_completed: Arc::new(AtomicU64::new(0)),
            semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
            probe_time: Arc::new(Mutex::new(WorkerProbeTime {
                srtt: 0.0,
                rttvar: 0.0,
                timeout: 3000.0,
            })),
        }
    }

    async fn maybe_wait_for_permit(&self) {
        if self.conf.max_concurrent_requests != 0 {
            self.semaphore.acquire().await.unwrap().forget();
        }
    }

    async fn maybe_release_permit(&self) {
        if self.conf.max_concurrent_requests != 0 {
            self.semaphore.add_permits(1);
        }
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

pub async fn run(tx: Sender<WorkerMessage>, conf: Conf) {
    let mut ws = WorkerState::new(conf, net::build_https_client());

    if !ws.conf.dataset.is_empty() {
        let mut dataset =
            EasyReader::new(File::open(Path::new(&ws.conf.dataset)).unwrap()).unwrap();

        while ws.conf.max_targets == 0 || ws.targets_count < ws.conf.max_targets {
            let target = if let Some(target) = get_next_dataset_target(&mut dataset).await {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            tokio::spawn(target_requests(tx.clone(), ws.clone(), target));

            ws.targets_count += 1;
        }
    } else {
        while ws.conf.max_targets == 0 || ws.targets_count < ws.conf.max_targets {
            let target = if let Some(target) = get_next_subnet_target(&ws.conf).await {
                target
            } else {
                // All the targets have been consumed
                break;
            };

            tokio::spawn(target_requests(tx.clone(), ws.clone(), target));

            ws.targets_count += 1;
        }
    };

    while ws.targets_completed.load(Ordering::SeqCst) < ws.targets_count {
        sleep(Duration::from_millis(500)).await;
    }

    tx.send(WorkerMessage::Shutdown).await.unwrap();
}
