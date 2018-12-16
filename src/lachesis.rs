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
use crate::db::DbMan;
use crate::worker::{
    self,
    LacMessage
};
use crate::detector;
use crate::stats::Stats;

#[derive(Clone, Debug)]
pub struct LacConf {
    pub definitions_paths: Vec<String>,
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<Vec<Ipv4AddrRange>>>,
    pub debug: bool,
    pub help: bool,
    pub max_targets: usize,
    pub print_records: bool
}

impl LacConf {
    pub fn default() -> LacConf {
        LacConf {
            definitions_paths: Vec::new(),
            definitions: Vec::new(),
            dataset: String::new(),
            subnets: Arc::new(Mutex::new(Vec::new())),
            debug: false,
            help: false,
            max_targets: 0,
            print_records: false
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub protocol: String,
    pub options: Options,
    pub service: Service,
    pub versions: Option<Versions>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub message: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub regex: String,
    pub log: bool
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Versions {
    pub semver: Option<SemverVersions>,
    pub regex: Option<Vec<RegexVersion>>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemverVersions {
    pub regex: String,
    pub ranges: Vec<RangeVersion>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeVersion {
    pub from: String,
    pub to: String,
    pub description: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegexVersion {
    pub regex: String,
    pub version: String,
    pub description: String
}

pub fn lachesis(conf: &LacConf) -> Result<(), i32> {
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
    let (tx, rx): (mpsc::Sender<LacMessage>, mpsc::Receiver<LacMessage>) = mpsc::channel();

    // Spawn worker
    let conf_inner = conf.clone();
    let thread = thread::spawn(move || {
        worker::run(tx, conf_inner);
    });

    // Manage worker's messages
    loop {
        let lr = match rx.try_recv() {
            Ok(lr) => lr,
            Err(_err) => continue
        };

        if lr.is_log() {
            stats.log_debug(lr.message);
            continue;
        }

        if lr.is_last_message() {
            break;
        }

        let host = if !lr.target.domain.is_empty() {
            lr.target.domain.clone()
        } else {
            lr.target.ip.clone()
        };

        let mut matching = false;
        if !lr.is_next_target_message() {
            stats.log(format!(
                "[{}][{}:{}] Received message from worker. Length: {}",
                lr.target.protocol.blue(),
                host.cyan(),
                lr.target.port.to_string().cyan(),
                lr.target.response.len().to_string().cyan()
            ));

            let responses = detector::detect(
                &host,
                lr.target.port,
                &lr.target.response,
                &conf.definitions
            );

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
        }

        stats.increment(lr.is_next_target_message(), &lr.target.protocol, matching);
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
