use std::{fmt::Debug, process::Termination};

use colored::Colorize;
use tokio::{
    runtime::Builder,
    sync::mpsc::{self, Receiver, Sender},
};

use crate::{
    conf::{self, Conf},
    db::DbMan,
    detector,
    stats::Stats,
    web::{self, UIMessage},
    worker::{self, PortsTarget, ReqTarget, WorkerMessage},
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

async fn handle_response_msg(conf: &Conf, stats: &mut Stats, dbm: &DbMan, target: ReqTarget) {
    stats.update_req_avg_time(target.time, &target.protocol);

    stats.log_response(&target);

    if !target.domain.is_empty() {
        if let Err(err) = dbm.update_or_insert_domain(&target.domain).await {
            stats.log_int_err(format!("Error while saving a domain in the db: {}", err));
        };
    }

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

            if let Err(err) = dbm.save_service(&res).await {
                stats.log_int_err(format!(
                    "Error while saving a matching service in the db: {}",
                    err
                ));
                continue;
            };

            // headless_chrome is unmaintained
            // browser::maybe_take_screenshot(&target, id);
        }
    }

    stats.increment_successful(&target.protocol, matching);
}

async fn handle_portstarget_msg(stats: &mut Stats, dbm: &DbMan, ports_target: PortsTarget) {
    stats.update_ports_stats(&ports_target);

    let open_ports = ports_target.open_ports();
    if !open_ports.is_empty() {
        stats.log_open_ports(&ports_target.ip, &open_ports);

        if let Err(err) = dbm
            .update_or_insert_ip_ports(&ports_target.ip, open_ports)
            .await
        {
            stats.log_int_err(format!(
                "Error while saving ip and ports in the db: {}",
                err
            ));
        };
    }
}

pub async fn run_worker(conf: &Conf) -> ExitCode {
    let mut stats = Stats::new(conf.max_targets);

    let dbm = match DbMan::init(&conf.db_conf).await {
        Ok(dbm) => dbm,
        Err(err) => {
            stats.log_int_err(format!("Db initialization error: {}", err));
            return ExitCode::Err;
        }
    };

    let (tx, mut rx): (Sender<WorkerMessage>, Receiver<WorkerMessage>) = mpsc::channel(100_000);

    let jhandle = tokio::spawn(worker::run(tx, conf.clone()));

    loop {
        let msg = match rx.recv().await {
            Some(msg) => msg,
            None => continue,
        };

        stats.update_avg_reqs_per_sec();

        match msg {
            WorkerMessage::PortsTarget(ports_target) => {
                handle_portstarget_msg(&mut stats, &dbm, ports_target).await;
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
                handle_response_msg(conf, &mut stats, &dbm, target).await;
                continue;
            }
            WorkerMessage::NextTarget => {
                stats.increment_targets();
                continue;
            }
            WorkerMessage::Shutdown => break,
        };
    }

    if let Err(e) = jhandle.await {
        stats.log_int_err(format!("The task being joined has panicked: {:?}", e));
    };

    stats.finish();

    ExitCode::Ok
}

async fn run_ui() -> ExitCode {
    let (tx, mut rx): (Sender<UIMessage>, Receiver<UIMessage>) = mpsc::channel(100);

    tokio::spawn(web::run(tx));

    loop {
        match rx.recv().await {
            Some(msg) => println!("{}", msg.message),
            None => continue,
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

    let rt = Builder::new_multi_thread().enable_all().build().unwrap();
    if conf.web_ui {
        rt.block_on(run_ui())
    } else {
        rt.block_on(run_worker(&conf))
    }
}
