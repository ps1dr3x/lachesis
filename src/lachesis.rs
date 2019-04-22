use std::{
    thread,
    sync::{
        mpsc,
        Arc,
        Mutex
    }
};
use serde_derive::{
    Serialize,
    Deserialize
};
use ipnet::Ipv4AddrRange;
use unindent::unindent;
use colored::Colorize;
use validator::Validate;
use crate::db::DbMan;
use crate::worker::{
    self,
    WorkerMessage
};
use crate::detector;
use crate::stats::Stats;
use crate::validators::{
    validate_definition,
    validate_protocol,
    validate_regex,
    validate_semver,
    validate_regex_ver
};
use crate::web::{
    self,
    UIMessage
};

#[derive(Clone, Debug, Validate)]
pub struct LacConf {
    #[validate]
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
    pub debug: bool,
    pub max_targets: usize,
    pub web_ui: bool
}

impl LacConf {
    pub fn default() -> LacConf {
        LacConf {
            definitions: Vec::new(),
            dataset: String::new(),
            subnets: Arc::new(Mutex::new((Vec::new(), 0))),
            debug: false,
            max_targets: 0,
            web_ui: false
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
#[validate(schema(function = "validate_definition"))]
pub struct Definition {
    pub name: String,
    #[validate(custom = "validate_protocol")]
    pub protocol: String,
    pub options: Options,
    #[validate]
    pub service: Service,
    #[validate]
    pub versions: Option<Versions>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub message: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Service {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub log: bool
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Versions {
    #[validate]
    pub semver: Option<SemverVersions>,
    #[validate(custom = "validate_regex_ver")]
    pub regex: Option<Vec<RegexVersion>>
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct SemverVersions {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    #[validate]
    pub ranges: Vec<RangeVersion>
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RangeVersion {
    #[validate(custom = "validate_semver")]
    pub from: String,
    #[validate(custom = "validate_semver")]
    pub to: String,
    pub description: String
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RegexVersion {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub version: String,
    pub description: String
}

pub fn run(conf: &LacConf) -> Result<(), i32> {
    if conf.web_ui {
        ui()
    } else {
        worker(conf)
    }
}

fn ui() -> Result<(), i32> {
    // Initialize the communication channel
    let (tx, rx): (mpsc::Sender<UIMessage>, mpsc::Receiver<UIMessage>) = mpsc::channel();

    // Run the Web UI
    thread::spawn(move || {
        web::run(tx);
    });

    // Manage Web UI's messages
    loop {
        let msg = match rx.recv() {
            Ok(msg) => msg,
            Err(_err) => continue
        };
        println!("{}", msg.message);
    }
}

fn worker(conf: &LacConf) -> Result<(), i32> {
    // Initialize the stats/logs manager
    let mut stats = Stats::new(conf.max_targets, conf.debug);

    // Initialize the embedded db manager
    let dbm = match DbMan::init() {
        Ok(dbm) => dbm,
        Err(err) => {
            stats.log(format!("\n[ERROR] Db initialization error: {}\n", err));
            return Err(1);
        }
    };

    // Initialize the communication channel
    let (tx, rx): (mpsc::Sender<WorkerMessage>, mpsc::Receiver<WorkerMessage>) = mpsc::channel();

    // Run the Worker
    let inner_conf = conf.clone();
    let thread = thread::spawn(move || {
        worker::run(&tx, inner_conf);
    });

    // Manage worker's messages
    loop {
        let msg = match rx.recv() {
            Ok(msg) => msg,
            Err(_err) => continue
        };

        match msg {
            WorkerMessage::Log(msg) => {
                stats.log_debug(msg);
                continue;
            },
            WorkerMessage::Shutdown => {
                break;
            },
            WorkerMessage::Response(target) => {
                let host = if !target.domain.is_empty() {
                    target.domain.clone()
                } else {
                    target.ip.clone()
                };

                stats.log(format!(
                    "[{}][{}:{}] Received a response. Length: {}",
                    target.protocol.blue(),
                    host.cyan(),
                    target.port.to_string().cyan(),
                    target.response.len().to_string().cyan()
                ));

                let responses = detector::detect(
                    &target.protocol,
                    &host,
                    target.port,
                    &target.response,
                    &conf.definitions
                );

                let mut matching = false;
                if !responses.is_empty() {
                    for res in responses {
                        if let Some(error) = res.error {
                            stats.log(error);
                            continue;
                        }

                        stats.log(unindent(format!("

                            ===
                            Matching service found: {}
                            Service: {}
                            Version: {}
                            Description: {}
                            ===

                        ",
                            res.host.green(),
                            res.service.green(),
                            res.version.green(),
                            res.description.green()).as_str())
                        );

                        match dbm.save_service(&res) {
                            Ok(_) => (),
                            Err(err) => {
                                stats.log(format!(
                                    "\n[{}] Error while saving a matching service in the embedded db: {}\n",
                                    "ERROR".red(), err
                                ));
                                return Err(1);
                            }
                        };
                        matching = true;
                    }
                }

                stats.increment(&target.protocol, matching);
            },
            WorkerMessage::NextTarget => {
                stats.increment_targets();
            }
        };
    }

    // Join the worker's thread
    thread.join().unwrap_or_else(|err| {
        stats.log(format!(
            "\n[{}] The thread being joined has panicked: {:?}\n",
            "ERROR".red(), err
        ))
    });

    stats.finish();
    Ok(())
}
