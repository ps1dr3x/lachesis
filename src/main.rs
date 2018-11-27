extern crate easy_reader;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate unindent;

mod utils;
mod detector;
mod db;
mod net;
mod stats;

use std::{
    thread,
    sync::mpsc
};
use unindent::unindent;
use net::LacMessage;
use detector::Detector;
use db::DbMan;
use stats::Stats;

fn usage() {
    println!("{}", unindent("

        Usage:

        lachesis --file dns.json [...optional arguments]

        Optional arguments:

        --threads NUM (default: 4)
        --max-targets NUM (default: 1000)
        --debug
        --print-records

    "));
}

fn lachesis() -> Result<(), i32> {
    println!("{}", unindent("

        
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-' 
                              v0.1.0

    "));

    // Check cli parameters
    let conf = match utils::get_cli_params() {
        Ok(conf) => conf,
        Err(err) => {
            println!("{}", err);
            usage();
            return Err(1);
        }
    };

    // --help option specified. Print usage and exit
    if conf.help {
        usage();
        return Ok(());
    }

    // --print-records option specified. Print records and exit
    if conf.print_records {
        let dbm = db::DbMan::new();
        let records = dbm.get_all_services().unwrap();
        if records.is_empty() {
            println!("Db is empty or not exists yet\n");
            return Ok(());
        }
        println!("{} records:\n", records.len());
        for rec in records {
            println!("{:?}", rec);
        }
        return Ok(());
    }

    // Read/validate definitions
    let definitions = match utils::read_validate_definitions() {
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
        let file_path = conf.file_path.clone();
        let definitions = definitions.clone();
        let thread = thread::spawn(move || {
            let mut worker = net::LacWorker::new(
                thread_tx,
                thread_id,
                file_path,
                definitions,
                if thread_id == 0 {
                    targets_per_thread + gap
                } else {
                    targets_per_thread
                }
            );
            worker.run();
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
                        ===

                    ",
                        res.host,
                        res.service,
                        res.version).as_str())
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

fn main() {
    ::std::process::exit(match lachesis() {
       Ok(_) => 0,
       Err(exit_code) => exit_code
    });
}
