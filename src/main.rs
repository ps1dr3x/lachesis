extern crate easy_reader;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate unindent;

mod utils;
mod detector;
mod db;
mod net;

use std::{
    thread,
    sync::mpsc
};
use unindent::unindent;
use net::LacResponse;
use detector::Detector;
use db::DbMan;

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
    let mut stats = utils::Stats::default();

    // Threads vector and communication channel
    let mut threads: Vec<thread::JoinHandle<()>> = Vec::with_capacity(conf.threads as usize);
    let (tx, rx): (mpsc::Sender<LacResponse>, mpsc::Receiver<LacResponse>) = mpsc::channel();

    // Spawn workers
    let targets_per_thread = (conf.max_targets as f32 / conf.threads as f32) as usize;
    for thread_id in 0..conf.threads {
        println!("Spawning new worker. ID: {}", thread_id);
        let thread_tx = tx.clone();
        let file_path = conf.file_path.clone();
        let definitions = definitions.clone();
        let debug = conf.debug;
        let thread = thread::spawn(move || {
            let mut worker = net::LacWorker::new(
                thread_tx,
                thread_id,
                file_path,
                definitions,
                targets_per_thread,
                debug
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

        if lr.last {
            running_threads -= 1;
            continue;
        }

        println!("Message from worker: {}", lr.thread_id);

        let mut matching = false;
        if !lr.unreachable && !lr.target.response.is_empty() {
            let mut detector = Detector::new(definitions.clone());
            detector.run(
                lr.target.host.clone(),
                lr.target.port,
                lr.target.response.clone()
            );

            if !detector.response.is_empty() {
                for res in detector.response {
                    println!("{}", unindent(format!("

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

                    let dbm: DbMan = DbMan::new();
                    dbm.save_service(res).unwrap();
                    matching = true;
                }
            }
        }

        stats.increment(lr.unreachable, lr.target.protocol.clone(), matching);
    }

    // Join all the threads
    for thread in threads {
        thread.join().expect(&format!("The thread being joined has panicked\n"));
    }

    // Print stats
    println!("{}", unindent(format!("

        ===== SCAN  COMPLETED =====
        
        Threads: {}
        Targets: {}
        Unreachables: {}
        Https: {}
        Http: {}
        Tcp/custom: {}
        Total requests: {}

        Matching services found: {}
        ===========================

    ",
        conf.threads,
        stats.targets,
        stats.unreachables,
        stats.requests_https,
        stats.requests_http,
        stats.requests_tcp_custom,
        stats.total_requests,
        stats.services_found).as_str())
    );

    Ok(())
}

fn main() {
    ::std::process::exit(match lachesis() {
       Ok(_) => 0,
       Err(exit_code) => exit_code
    });
}
