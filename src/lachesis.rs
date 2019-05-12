use std::{
    thread,
    sync::{
        mpsc,
        Arc,
        Mutex
    },
    time,
    fs::File,
    io::prelude::Write,
    path::Path
};

use serde_derive::{
    Serialize,
    Deserialize
};
use ipnet::Ipv4AddrRange;
use unindent::unindent;
use colored::Colorize;
use validator::Validate;
use headless_chrome::{
    browser,
    Browser,
    LaunchOptionsBuilder,
    protocol::page::ScreenshotFormat
};

use crate::db::DbMan;
use crate::worker::{
    self,
    WorkerMessage,
    Target
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
use crate::utils::format_host;

#[derive(Clone, Debug, Validate)]
pub struct LacConf {
    #[validate]
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
    pub user_agent: String,
    pub max_targets: usize,
    pub debug: bool,
    pub web_ui: bool
}

impl LacConf {
    pub fn default() -> LacConf {
        LacConf {
            definitions: Vec::new(),
            dataset: String::new(),
            subnets: Arc::new(Mutex::new((Vec::new(), 0))),
            user_agent: String::new(),
            max_targets: 0,
            debug: false,
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

fn maybe_take_screenshot(target: &Target, id: String) {
    let target = target.clone();
    thread::spawn(move || {
        if target.protocol != "https"
        && target.protocol != "http" {
            return;
        }

        let browser_path = match browser::default_executable() {
            Ok(path) => path,
            Err(_e) => return
        };

        let browser_options = LaunchOptionsBuilder::default()
            .path(Some(browser_path))
            .build().unwrap();
        let browser = Browser::new(browser_options).unwrap();
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
                let jpeg_data = tab.capture_screenshot(
                    ScreenshotFormat::JPEG(Some(75)),
                    None,
                    true
                ).unwrap();
                let mut file = File::create(
                    Path::new("data/screenshots/").join(&id)
                ).unwrap();
                file.write_all(&jpeg_data).unwrap();
            },
            Err(_e) => return
        };
    });
}

fn manage_worker_response(
    conf: &LacConf,
    stats: &mut Stats,
    dbm: &DbMan,
    target: Target
) -> Result<(), i32> {
    stats.log(format!(
        "[{}][{}:{}] Received a response. Length: {}",
        target.protocol.blue(),
        format_host(&target).cyan(),
        target.port.to_string().cyan(),
        target.response.len().to_string().cyan()
    ));

    let responses = detector::detect(
        &target,
        &conf.definitions
    );

    let mut matching = false;
    if !responses.is_empty() {
        for res in responses {
            if let Some(error) = res.error {
                stats.log(error);
                continue;
            }

            matching = true;

            stats.log(unindent(format!("
                ===
                Matching service found: {}
                Service: {}
                Version: {}
                Description: {}
                ===
            ",
                format_host(&res.target).green(),
                res.service.green(),
                res.version.green(),
                res.description.green()).as_str())
            );

            let id = match dbm.save_service(&res) {
                Ok(id) => id.to_string(),
                Err(err) => {
                    stats.log(format!(
                        "\n[{}] Error while saving a matching service in the embedded db: {}\n",
                        "ERROR".red(), err
                    ));
                    return Err(1);
                }
            };

            maybe_take_screenshot(&target, id);
        }
    }

    stats.increment(&target.protocol, matching);

    Ok(())
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
                match manage_worker_response(conf, &mut stats, &dbm, target) {
                    Ok(()) => (),
                    Err(code) => return Err(code)
                }
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

pub fn run(conf: &LacConf) -> Result<(), i32> {
    if conf.web_ui {
        ui()
    } else {
        worker(conf)
    }
}
