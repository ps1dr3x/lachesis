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
use utils::LacConf;

struct Stats {
    targets: usize,
    requests: usize,
    unreachables: usize,
    requests_https: usize,
    requests_http: usize,
    requests_tcp_custom: usize,
    services_found: usize
}

impl Stats {
    fn default() -> Stats {
        Stats {
            targets: 0,
            requests: 0,
            unreachables: 0,
            requests_https: 0,
            requests_http: 0,
            requests_tcp_custom: 0,
            services_found: 0
        }
    }

    pub fn increment(&mut self, lr: &LacResponse) {
        self.targets += &lr.targets.len();

        for target in &lr.targets {
            if target.unreachable {
                self.unreachables += 1;
                continue;
            }
            self.requests_https += target.https.len();
            self.requests_http += target.http.len();
            self.requests_tcp_custom += target.tcp_custom.len();
            self.requests += self.requests_https +
                self.requests_http +
                self.requests_tcp_custom;
            self.services_found += target.matching as usize;
        }
    }
}

fn usage() {
    println!("{}", unindent("

        Usage:

        lachesis --file dns.json [...optional arguments]

        Optional arguments:

        --threads NUM (default: 250)
        --max-targets NUM (default: 10000)
        --debug
        --print-records

    "));
}

fn main() {
    println!("{}", unindent("

        
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-' 
                              v0.1.0

    "));

    // Check cli parameters
    let conf = utils::get_cli_params();
    if conf.is_err() {
        println!("{}", conf.err().unwrap());
        usage();
        ::std::process::exit(1);
    }
    let conf: LacConf = conf.unwrap();

    // --help option specified. Print usage and exit
    if conf.help {
        usage();
        ::std::process::exit(0);
    }

    // --print-records option specified. Print records and exit
    if conf.print_records {
        let dbm = db::DbMan::new();
        let records = dbm.get_all_services().unwrap();
        if records.is_empty() {
            println!("Db is empty or not created yet\n");
            return;
        }
        println!("{} records:\n", records.len());
        for rec in records {
            println!("{:?}", rec);
        }
        ::std::process::exit(0);
    }

    // --threads value can't be greater than --max-targets value
    if conf.threads > conf.max_targets {
        println!("The number of threads can't be greater than the number of max targets\n");
        ::std::process::exit(1);
    }

    // Read/validate definitions
    let definitions = utils::read_definitions();
    if definitions.is_err() {
        println!("Definitions validation failed. Error:\n{}", definitions.unwrap_err());
        ::std::process::exit(1);
    }
    let definitions = definitions.unwrap();

    // Some stats
    let mut stats = Stats::default();

    // Threads vector and communication channel
    let mut threads: Vec<thread::JoinHandle<()>> = Vec::with_capacity(conf.threads);
    let (tx, rx): (mpsc::Sender<LacResponse>, mpsc::Receiver<LacResponse>) = mpsc::channel();

    // Spawn threads
    for thread_id in 0..conf.threads {
        let thread = net::lac_worker(
            tx.clone(),
            thread_id as u16,
            conf.file_path.clone(),
            definitions.clone(),
            (conf.max_targets / conf.threads) as usize,
            conf.debug
        );
        threads.push(thread);
    }

    // Wait for the end of all threads
    for n in 0..conf.threads {
        let lr = rx.recv().unwrap();
        if conf.debug { println!("Worker {} ended [{}/{}]\n", lr.thread_id, n, conf.threads - 1); }

        stats.increment(&lr);
    }
    for thread in threads {
        thread.join().expect(&format!("The thread being joined has panicked\n"));
    }

    // Print results
    println!("{}", unindent(format!("
        === SCAN TEST COMPLETED ===
        
        Threads: {}
        Targets: {}
        Requests: {}
        Unreachables: {}
        Https: {}
        Http: {}
        Tcp/custom: {}

        Matching services found: {}
        ===========================
    ",
        conf.threads,
        stats.targets,
        stats.requests,
        stats.unreachables,
        stats.requests_https,
        stats.requests_http,
        stats.requests_tcp_custom,
        stats.services_found).as_str())
    );
}
