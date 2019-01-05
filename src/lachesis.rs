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
    LacMessage
};
use crate::detector;
use crate::stats::Stats;
use crate::utils::{
    validate_definition,
    validate_protocol,
    validate_regex,
    validate_semver,
    validate_regex_ver
};

#[derive(Clone, Debug)]
pub struct LacConf {
    pub definitions_paths: Vec<String>,
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
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
            subnets: Arc::new(Mutex::new((Vec::new(), 0))),
            debug: false,
            help: false,
            max_targets: 0,
            print_records: false
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
        worker::run(&tx, conf_inner);
    });

    // Manage worker's messages
    loop {
        let lr = match rx.recv() {
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
                "[{}][{}:{}] Received a response. Length: {}",
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
