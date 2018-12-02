use std::{
    thread,
    sync::mpsc
};
use db::DbMan;
use worker::{
    LacWorker,
    LacMessage
};
use utils;
use detector::Detector;
use stats::Stats;
use unindent::unindent;

#[derive(Debug)]
pub struct LacConf {
    pub definitions: Vec<String>,
    pub dataset: String,
    pub debug: bool,
    pub help: bool,
    pub threads: u16,
    pub max_targets: usize,
    pub print_records: bool
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

pub fn lachesis(conf: LacConf) -> Result<(), i32> {
    // Read & validate definitions
    let definitions = match utils::read_validate_definitions(conf.definitions) {
        Ok(definitions) => definitions,
        Err(err) => {
            println!("Definitions validation failed. Error: {}", err);
            return Err(1);
        }
    };

    // Some stats
    let mut stats = Stats::new(conf.threads, conf.max_targets, conf.debug);

    // Threads vector and communication channel
    let mut threads: Vec<thread::JoinHandle<()>> = Vec::with_capacity(conf.threads as usize);
    let (tx, rx): (mpsc::Sender<LacMessage>, mpsc::Receiver<LacMessage>) = mpsc::channel();

    // Spawn workers
    let targets_per_thread = (conf.max_targets as f32 / conf.threads as f32) as usize;
    let gap = conf.max_targets - (targets_per_thread * conf.threads as usize);
    for thread_id in 0..conf.threads {
        stats.log(format!("[+] Spawning new worker. ID: {}", thread_id));
        let thread_tx = tx.clone();
        let dataset = conf.dataset.clone();
        let definitions = definitions.clone();
        let thread = thread::spawn(move || {
            LacWorker::new(
                thread_tx,
                thread_id,
                dataset,
                definitions,
                if thread_id == 0 {
                    targets_per_thread + gap
                } else {
                    targets_per_thread
                }
            ).run();
        });
        threads.push(thread);
    }

    // Manage workers messages
    let mut running_threads = conf.threads;
    while running_threads > 0 {
        let lr = match rx.try_recv() {
            Ok(lr) => lr,
            Err(_err) => {
                continue;
            }
        };

        if lr.is_log() {
            stats.log_debug(lr.message);
            continue;
        }

        if lr.is_last_message() {
            stats.log(format!("[-] Shutting down worker: {}", lr.thread_id));
            running_threads -= 1;
        }

        let mut matching = false;
        if !lr.is_unreachable() && !lr.is_next_target_message() {
            stats.log(format!(
                "[{}][{}:{}] Message from worker: {} length: {}",
                lr.target.protocol,
                lr.target.host,
                lr.target.port,
                lr.thread_id,
                lr.target.response.len()
            ));

            let mut detector = Detector::new(definitions.clone());
            let responses = detector.run(
                lr.target.host.clone(),
                lr.target.port,
                lr.target.response.clone()
            );

            if !responses.is_empty() {
                for res in responses {
                    stats.log(unindent(format!("

                        ===
                        Matching service found: {}
                        Service: {}
                        Version: {}
                        Description: {}
                        ===
                    ",
                        res.host,
                        res.service,
                        res.version,
                        res.description).as_str())
                    );

                    let dbm = DbMan::new();
                    dbm.save_service(res).unwrap();
                    matching = true;
                }
            }
        }

        stats.increment(lr.is_next_target_message(), lr.is_unreachable(), lr.target.protocol, matching);
    }

    // Print stats
    stats.finish();

    // Join all the threads
    for thread in threads {
        thread.join().expect(&format!("The thread being joined has panicked"));
    }

    Ok(())
}