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
    path::Path,
    fs::File,
    thread,
    sync::mpsc
};
use easy_reader::EasyReader;
use unindent::unindent;
use net::LacResponse;
use utils::LacConf;

struct Stats {
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
            requests: 0,
            unreachables: 0,
            requests_https: 0,
            requests_http: 0,
            requests_tcp_custom: 0,
            services_found: 0
        }
    }

    pub fn increment(&mut self, lr: &LacResponse) {
        if lr.unreachable { self.unreachables += 1; }
        if !lr.https.is_empty() { self.requests_https += lr.https.len(); }
        if !lr.http.is_empty() { self.requests_http += lr.http.len(); }
        if !lr.tcp_custom.is_empty() { self.requests_tcp_custom += lr.tcp_custom.len(); }
    }
}

fn usage() {
    println!("{}", unindent("

        Usage:

        lachesis --file dns.json [...optional arguments]

        Optional arguments:

        --threads NUM (default: 250)
        --max-requests NUM (default: 10000)
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

    // Read/validate definitions
    let definitions = utils::read_definitions();
    if definitions.is_err() {
        println!("Definitions validation failed. Error:\n{}", definitions.unwrap_err());
        ::std::process::exit(1);
    }
    let definitions = definitions.unwrap();

    // Some stats
    let mut stats = Stats::default();

    // Open dns records file and instantiate the reader
    let dns_records_file_path: &Path = Path::new(conf.file_path.as_str());
    let dns_records_file: File = File::open(dns_records_file_path).unwrap();
    let mut easy_reader: EasyReader = EasyReader::new(dns_records_file).unwrap();

    // Threads vector and communication channel
    let n_threads = conf.threads;
    let mut threads: Vec<thread::JoinHandle<()>> = Vec::with_capacity(n_threads);
    let (tx, rx): (mpsc::Sender<LacResponse>, mpsc::Receiver<LacResponse>) = mpsc::channel();

    let n_requests = conf.max_requests;
    while stats.requests < n_requests {
        // Pick a random dns record
        let line_str: String = easy_reader.random_line().unwrap();
        let line_json: serde_json::Value = serde_json::from_str(&line_str).unwrap();

        // Exclude records which are not of type A
        if line_json["type"].as_str().unwrap() != "a" {
            continue;
        }

        // If there are free slots, pick a thread_id (index of the threads vector) for a new thread
        let thread_id: u16 = if threads.len() < n_threads {
            threads.len() as u16
        } else {
            // Otherwise wait the end of one of the active threads
            let lr: LacResponse = rx.recv().unwrap(); 
            if conf.debug { println!("Request in thread {} completed\n", lr.thread_id); }

            // Increment results variables
            stats.increment(&lr);

            // And use its thread_id for a new thread
            lr.thread_id
        };

        // Spawn a new thread
        let thread_tx: mpsc::Sender<LacResponse> = tx.clone();
        let thread: thread::JoinHandle<()> = net::lac_request_thread(
            thread_tx,
            thread_id,
            definitions.clone(),
            line_json["name"].as_str().unwrap().to_string(),
            conf.debug
        );

        // Push the new thread into the threads vector
        if threads.len() < n_threads {
            threads.push(thread);
        } else {
            threads[thread_id as usize] = thread;
        }

        // Total requests counter (printed every 500 request)
        stats.requests += 1;
        println!("Requests: {}\n", stats.requests);
        if stats.requests % 500 == 0 {
            println!("Requests: {}\n", stats.requests);
        }
    }

    // Wait for the end of all threads
    let mut id: u16 = 0;
    for thread in threads {
        thread.join().expect(&format!("The thread being joined has panicked. ID: {}\n", id));

        let lr: LacResponse = rx.recv().unwrap(); 
        if conf.debug { println!("Request in thread {} completed\n", lr.thread_id); }

        stats.increment(&lr);

        id += 1;
    }

    // Print results
    println!("{}", unindent(format!("
        === SCAN TEST COMPLETED ===
        
        Threads: {}
        Requests: {}
        Unreachables: {}
        Https: {}
        Http: {}
        Tcp/custom: {}

        Matching services found: {}
        ===========================
    ",
        n_threads,
        stats.requests,
        stats.unreachables,
        stats.requests_https,
        stats.requests_http,
        stats.requests_tcp_custom,
        stats.services_found).as_str())
    );
}
