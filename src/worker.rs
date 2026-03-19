use std::{
    collections::HashSet,
    fs::File,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use easy_reader::EasyReader;
use serde_derive::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc::Sender, Mutex, Semaphore},
    time::{sleep, Duration},
};

use crate::{
    conf::{Conf, Definition},
    net::{self, HttpsOptions},
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
    let mut ports_target = PortsTarget {
        ip: ip.clone(),
        ports: Vec::new(),
    };
    for port in unique_ports {
        ws.maybe_wait_for_permit().await;

        let now = Instant::now();
        let timeout = ws.probe_time.lock().await.timeout;
        let port_target = net::test_port(ip.clone(), port, timeout as u64).await;

        if port_target.status != PortStatus::Open {
            open_ports.remove(&port);
        }

        ports_target.ports.push(port_target);

        let rtt = now.elapsed().as_millis() as f32;
        let mut pt = ws.probe_time.lock().await;
        pt.timeout = estimate_timeout(pt.srtt, rtt, pt.rttvar);

        ws.maybe_release_permit().await;
    }

    // If the receiver has been dropped we're shutting down, just return.
    let _ = tx.send(WorkerMessage::PortsTarget(ports_target)).await;

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

    let mut http_s_unique_opts = HashSet::new();
    for def in &ws.conf.definitions {
        match def.protocol.as_str() {
            "http/s" => {
                // Avoid duplicate requests (same port, method, path, headers and payload)
                for port in &def.options.ports {
                    if open_ports.contains(port) {
                        let options = HttpsOptions {
                            method: def
                                .options
                                .method
                                .clone()
                                .unwrap_or_else(|| "GET".to_string()),
                            path: def.options.path.clone().unwrap_or_else(|| "/".to_string()),
                            headers: def.options.headers.clone().unwrap_or_default(),
                            payload: def
                                .options
                                .payload
                                .clone()
                                .unwrap_or_else(|| "".to_string()),
                        };
                        http_s_unique_opts.insert((*port, options));
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

                    net::tcp_custom(
                        tx.clone(),
                        target,
                        def.options.payload.clone().unwrap(),
                        ws.conf.req_timeout,
                        ws.conf.max_response_size,
                    )
                    .await;

                    ws.maybe_release_permit().await;
                }
            }
            // Protocol field is already validated when conf is loaded
            _ => (),
        }
    }

    for protocol in ["https", "http"].iter() {
        for (port, opts) in &http_s_unique_opts {
            if (*port == 80 && *protocol == "https") || (*port == 443 && *protocol == "http") {
                continue;
            }

            ws.maybe_wait_for_permit().await;

            let mut target = target.clone();
            target.protocol = protocol.to_string();
            target.port = *port;
            target.time = Instant::now();

            net::http_s(
                tx.clone(),
                ws.https_client.clone(),
                target,
                opts.clone(),
                ws.conf.user_agent.clone(),
                ws.conf.req_timeout,
                ws.conf.max_response_size,
            )
            .await;

            ws.maybe_release_permit().await;
        }
    }

    ws.targets_completed.fetch_add(1, Ordering::Relaxed);
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
    // Bound the search to avoid an infinite loop when the dataset has no A records
    // or only a very small fraction of them.
    const MAX_TRIES: usize = 10_000;
    for _ in 0..MAX_TRIES {
        let line_str = match dataset.random_line() {
            Ok(Some(l)) => l,
            _ => return None,
        };
        let dataset_record: DatasetRecord = match serde_json::from_str(&line_str) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if dataset_record.record_type == "a" {
            return Some(ReqTarget::new(dataset_record.name, dataset_record.value));
        }
    }
    None
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

    ip.map(|ip| ReqTarget::new(String::new(), ip.to_string()))
}

#[derive(Debug, Clone)]
struct WorkerProbeTime {
    srtt: f32,
    rttvar: f32,
    timeout: f32,
}

#[derive(Debug, Clone)]
struct WorkerState {
    conf: Conf,
    https_client: reqwest::Client,
    targets_count: u64,
    targets_completed: Arc<AtomicU64>,
    semaphore: Arc<Semaphore>,
    probe_time: Arc<Mutex<WorkerProbeTime>>,
}

impl WorkerState {
    fn new(conf: Conf, https_client: reqwest::Client) -> Self {
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
    pub port: u16,
    pub status: PortStatus,
    pub time: Instant,
}

#[derive(Debug, Clone)]
pub struct PortsTarget {
    pub ip: String,
    pub ports: Vec<PortTarget>,
}

impl PortsTarget {
    pub fn open_ports(&self) -> Vec<u16> {
        let mut open_ports = Vec::new();
        for port in &self.ports {
            if port.status == PortStatus::Open {
                open_ports.push(port.port);
            }
        }
        open_ports
    }
}

#[derive(Debug, Clone)]
pub enum WorkerMessage {
    PortsTarget(PortsTarget),
    Response(ReqTarget),
    Fail(ReqTarget, String, Option<String>),
    Timeout(ReqTarget),
    NextTarget,
    Shutdown,
}

pub async fn run(tx: Sender<WorkerMessage>, conf: Conf) {
    let mut ws = WorkerState::new(conf, net::build_https_client());

    let dataset_path = if !ws.conf.dataset.is_empty() {
        ws.conf.dataset.clone()
    } else {
        // In subnet mode a dataset file isn't used, but EasyReader needs something to
        // open. Use the bundled test file as a harmless placeholder.
        "./resources/test-dataset.json".to_string()
    };
    let mut dataset = match File::open(&dataset_path).map(EasyReader::new) {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("[ERROR] Failed to initialise dataset reader: {}", e);
            let _ = tx.send(WorkerMessage::Shutdown).await;
            return;
        }
        Err(e) => {
            eprintln!(
                "[ERROR] Failed to open dataset file '{}': {}",
                dataset_path, e
            );
            let _ = tx.send(WorkerMessage::Shutdown).await;
            return;
        }
    };

    while ws.conf.max_targets == 0 || ws.targets_count < ws.conf.max_targets {
        let target = if !ws.conf.dataset.is_empty() {
            get_next_dataset_target(&mut dataset).await
        } else {
            get_next_subnet_target(&ws.conf).await
        };

        let target = match target {
            Some(target) => target,
            None => break, // All the targets have been consumed
        };

        tokio::spawn(target_requests(tx.clone(), ws.clone(), target));

        ws.targets_count += 1;
    }

    while ws.targets_completed.load(Ordering::Relaxed) < ws.targets_count {
        sleep(Duration::from_millis(500)).await;
    }

    tx.send(WorkerMessage::Shutdown).await.unwrap();
}
