use std::{
    sync::{
        mpsc,
        mpsc::{channel, Receiver, Sender},
    },
    thread,
    fmt::Debug, process::Termination,
};

use colored::Colorize;

use crate::{
    conf::{
        self,
        Conf,
    },
    db::DbMan,
    detector,
    stats::Stats,
    utils::format_host,
    web::{self, UIMessage},
    worker::{self, Target, WorkerMessage},
    browser,
};

#[derive(Debug, PartialEq)]
pub enum ExitCode {
    Ok,
    Err,
}

impl Termination for ExitCode {
    fn report(self) -> i32 {
        match self {
            ExitCode::Ok => 0,
            ExitCode::Err => 1,
        }
    }
}

fn handle_worker_response(
    conf: &Conf,
    stats: &mut Stats,
    dbm: &DbMan,
    target: Target,
) -> ExitCode {
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
                    return ExitCode::Err;
                }
            };

            browser::maybe_take_screenshot(&target, id);
        }
    }

    stats.increment_successful(&target.protocol, matching);

    ExitCode::Ok
}

fn run_worker(conf: &Conf) -> ExitCode {
    let mut stats = Stats::new(conf.max_targets);

    let dbm = match DbMan::init() {
        Ok(dbm) => dbm,
        Err(err) => {
            stats.log_err(format!("Db initialization error: {}", err));
            return ExitCode::Err;
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
                if conf.debug {
                    stats.log_err(msg);
                }
                stats.increment_timedout(&protocol);
                continue;
            }
            WorkerMessage::Response(target) => {
                stats.update_avg_time(target.time, &target.protocol);
                if handle_worker_response(conf, &mut stats, &dbm, target) == ExitCode::Err {
                    return ExitCode::Err;
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
    
    ExitCode::Ok
}

fn run_ui() -> ExitCode {
    let (tx, rx): (Sender<UIMessage>, Receiver<UIMessage>) = channel();

    thread::spawn(move || web::run(tx));

    loop {
        match rx.recv() {
            Ok(msg) => println!("{}", msg.message),
            Err(_) => continue,
        };
    }
}

pub fn run() -> ExitCode {
    let conf = match conf::load() {
        Ok(conf) => conf,
        Err(err) => {
            println!("\n[{}] {}", "ERROR".red(), err);
            return ExitCode::Err;
        }
    };

    if conf.web_ui {
        run_ui()
    } else {
        run_worker(&conf)
    }
}
