use std::{fmt::Debug, process::Termination, sync::mpsc as sync_mpsc, thread};

use colored::Colorize;
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::{
    browser,
    conf::{self, Conf},
    db::DbMan,
    detector,
    stats::Stats,
    web::{self, UIMessage},
    worker::{self, ReqTarget, WorkerMessage},
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
    target: ReqTarget,
) {
    stats.update_req_avg_time(target.time, &target.protocol);

    stats.log_response(&target);

    let det_responses = detector::detect(&target, &conf.definitions);

    let mut matching = false;
    if !det_responses.is_empty() {
        for res in det_responses {
            if let Some(error) = res.error {
                stats.log_int_err(error);
                continue;
            }

            matching = true;

            stats.log_match(&res);

            let id = match dbm.save_service(&res) {
                Ok(id) => id.to_string(),
                Err(err) => {
                    stats.log_int_err(format!(
                        "Error while saving a matching service in the embedded db: {}",
                        err
                    ));
                    continue;
                }
            };

            browser::maybe_take_screenshot(&target, id);
        }
    }

    stats.increment_successful(&target.protocol, matching);
}

async fn run_worker(conf: &Conf) -> ExitCode {
    let mut stats = Stats::new(conf.max_targets);

    let dbm = match DbMan::init() {
        Ok(dbm) => dbm,
        Err(err) => {
            stats.log_int_err(format!("Db initialization error: {}", err));
            return ExitCode::Err;
        }
    };

    let (tx, mut rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = mpsc::channel(100000);

    let in_conf = conf.clone();
    let thread = thread::spawn(move || worker::run(tx, in_conf));

    loop {
        let msg = match rx.recv().await {
            Some(msg) => msg,
            None => continue,
        };

        stats.update_avg_reqs_per_sec();

        match msg {
            WorkerMessage::PortTarget(port_target) => {
                stats.update_ports_stats(port_target.status, port_target.time);
                continue;
            }
            WorkerMessage::Fail(target, error_context, error) => {
                if conf.debug {
                    stats.log_fail(&target, error_context, error);
                }
                stats.increment_failed(&target.protocol);
                continue;
            }
            WorkerMessage::Timeout(target) => {
                if conf.debug {
                    stats.log_timeout(&target);
                }
                stats.increment_timedout(&target.protocol);
                continue;
            }
            WorkerMessage::Response(target) => {
                handle_worker_response(conf, &mut stats, &dbm, target);
                continue;
            }
            WorkerMessage::NextTarget => {
                stats.increment_targets();
                continue;
            }
            WorkerMessage::Shutdown => break,
        };
    }

    if let Err(e) = thread.join() {
        stats.log_int_err(format!("The thread being joined has panicked: {:?}", e));
    };

    stats.finish();

    ExitCode::Ok
}

fn run_ui() -> ExitCode {
    let (tx, rx): (sync_mpsc::Sender<UIMessage>, sync_mpsc::Receiver<UIMessage>) =
        sync_mpsc::channel();

    thread::spawn(move || web::run(tx));

    loop {
        match rx.recv() {
            Ok(msg) => println!("{}", msg.message),
            Err(_) => continue,
        };
    }
}

#[tokio::main]
pub async fn run() -> ExitCode {
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
        run_worker(&conf).await
    }
}
