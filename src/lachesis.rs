use std::{
    fs::File,
    io::prelude::Write,
    path::Path,
    sync::{
        mpsc,
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread, time,
};

use colored::Colorize;
use headless_chrome::{browser, protocol::page::ScreenshotFormat, Browser, LaunchOptionsBuilder};
use ipnet::Ipv4AddrRange;
use serde_derive::{Deserialize, Serialize};
use validator::Validate;

use crate::db::DbMan;
use crate::detector;
use crate::stats::Stats;
use crate::utils::format_host;
use crate::validators::{
    validate_definition, validate_protocol, validate_regex, validate_regex_ver, validate_semver,
};
use crate::web::{self, UIMessage};
use crate::worker::{self, Target, WorkerMessage};

#[derive(Clone, Debug, Validate)]
pub struct LacConf {
    #[validate]
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
    pub user_agent: String,
    pub max_targets: u64,
    pub req_timeout: u64,
    pub debug: bool,
    pub web_ui: bool,
}

impl LacConf {
    pub fn default() -> LacConf {
        LacConf {
            definitions: Vec::new(),
            dataset: String::new(),
            subnets: Arc::new(Mutex::new((Vec::new(), 0))),
            user_agent: String::new(),
            max_targets: 0,
            req_timeout: 10,
            debug: false,
            web_ui: false,
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
    pub versions: Option<Versions>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Service {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub log: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Versions {
    #[validate]
    pub semver: Option<SemverVersions>,
    #[validate(custom = "validate_regex_ver")]
    pub regex: Option<Vec<RegexVersion>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct SemverVersions {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    #[validate]
    pub ranges: Vec<RangeVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RangeVersion {
    #[validate(custom = "validate_semver")]
    pub from: String,
    #[validate(custom = "validate_semver")]
    pub to: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RegexVersion {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub version: String,
    pub description: String,
}

fn maybe_take_screenshot(target: &Target, id: String) {
    let target = target.clone();
    thread::spawn(move || {
        if target.protocol != "https" && target.protocol != "http" {
            return;
        }

        let browser_path = match browser::default_executable() {
            Ok(path) => path,
            Err(_e) => return,
        };

        let browser_options = LaunchOptionsBuilder::default()
            .path(Some(browser_path))
            .build()
            .unwrap();
        let browser = match Browser::new(browser_options) {
            Ok(b) => b,
            Err(_e) => return,
        };
        browser.wait_for_initial_tab().unwrap();
        let tab = browser.new_tab().unwrap();

        let host = format!(
            "{}://{}:{}",
            target.protocol,
            if !target.domain.is_empty() {
                target.domain
            } else {
                target.ip
            },
            target.port
        );
        match tab.navigate_to(&host) {
            Ok(tab) => {
                thread::sleep(time::Duration::from_secs(10));
                let jpeg_data = tab
                    .capture_screenshot(ScreenshotFormat::JPEG(Some(75)), None, true)
                    .unwrap();
                let mut file =
                    File::create(Path::new("data/screenshots/").join(&(id + ".jpg"))).unwrap();
                file.write_all(&jpeg_data).unwrap();
            }
            Err(_e) => {}
        };
    });
}

fn handle_worker_response(
    conf: &LacConf,
    stats: &mut Stats,
    dbm: &DbMan,
    target: Target,
) -> Result<(), i32> {
    stats.log_info(format!(
        "[{}][{}:{}] Received a response. Length: {}",
        target.protocol.to_uppercase().blue(),
        format_host(&target).cyan(),
        target.port.to_string().cyan(),
        target.response.len().to_string().cyan()
    ));

    let responses = detector::detect(&target, &conf.definitions);

    let mut matching = false;
    if !responses.is_empty() {
        for res in responses {
            if let Some(error) = res.error {
                stats.log_err(error);
                continue;
            }

            matching = true;

            stats.log_match(&res);

            let id = match dbm.save_service(&res) {
                Ok(id) => id.to_string(),
                Err(err) => {
                    stats.log_err(format!(
                        "Error while saving a matching service in the embedded db: {}",
                        err
                    ));
                    return Err(1);
                }
            };

            maybe_take_screenshot(&target, id);
        }
    }

    stats.increment_successful(&target.protocol, matching);

    Ok(())
}

fn run_worker(conf: &LacConf) -> Result<(), i32> {
    let mut stats = Stats::new(conf.max_targets);

    let dbm = match DbMan::init() {
        Ok(dbm) => dbm,
        Err(err) => {
            stats.log_err(format!("Db initialization error: {}", err));
            return Err(1);
        }
    };

    let (tx, rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = mpsc::channel();

    let in_conf = conf.clone();
    let thread = thread::spawn(move || worker::run(tx, in_conf));

    loop {
        let msg = match rx.recv() {
            Ok(msg) => msg,
            Err(_) => continue,
        };

        match msg {
            WorkerMessage::Error(msg, protocol) => {
                if conf.debug {
                    stats.log_err(msg);
                }
                stats.increment_failed(&protocol);
                continue;
            }
            WorkerMessage::Timeout(msg, protocol) => {
                stats.log_err(msg);
                stats.increment_timedout(&protocol);
                continue;
            }
            WorkerMessage::Response(target) => {
                stats.update_avg_time(target.time, &target.protocol);
                if let Err(code) = handle_worker_response(conf, &mut stats, &dbm, target) {
                    return Err(code);
                }
            }
            WorkerMessage::NextTarget => stats.increment_targets(),
            WorkerMessage::Shutdown => break,
        };
    }

    if let Err(e) = thread.join() {
        stats.log_err(format!("The thread being joined has panicked: {:?}", e));
    };

    stats.finish();
    Ok(())
}

fn run_ui() -> Result<(), i32> {
    let (tx, rx): (Sender<UIMessage>, Receiver<UIMessage>) = channel();

    thread::spawn(move || web::run(tx));

    loop {
        match rx.recv() {
            Ok(msg) => println!("{}", msg.message),
            Err(_) => continue,
        };
    }
}

pub fn run(conf: &LacConf) -> Result<(), i32> {
    if conf.web_ui {
        run_ui()
    } else {
        run_worker(conf)
    }
}
