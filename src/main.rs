extern crate easy_reader;
extern crate serde_json;
extern crate reqwest;
extern crate unindent;
extern crate time;

mod utils;
mod detector;
mod db;
mod net;

use std::path::Path;
use std::fs::File;
use easy_reader::EasyReader;
use std::thread;
use std::sync::mpsc;
use unindent::unindent;
use net::LacResponse;
use utils::LacConf;

struct Stats {
    requests: usize,
    requests_errors: usize,
    requests_https: usize,
    requests_http: usize,
    requests_status_not_ok: usize,
    services_found: usize
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
        return;
    }
    let conf: LacConf = conf.unwrap();

    // --help option specified. Print usage and exit
    if conf.help {
        usage();
        return;
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
        return;
    }

    // Some stats
    let mut stats = Stats {
        requests: 0,
        requests_errors: 0,
        requests_https: 0,
        requests_http: 0,
        requests_status_not_ok: 0,
        services_found: 0
    };

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
            let wr: LacResponse = rx.recv().unwrap(); 
            if conf.debug { println!("Request in thread {} completed\n", wr.thread_id); }

            // Increment results variables
            if wr.is_request_error { stats.requests_errors += 1; }
            if wr.is_https { stats.requests_https += 1; }
            if wr.is_http { stats.requests_http += 1; }
            if wr.is_status_not_ok { stats.requests_status_not_ok += 1; }

            // And use its thread_id for a new thread
            wr.thread_id
        };

        // Spawn a new thread
        let thread_tx: mpsc::Sender<LacResponse> = tx.clone();
        let thread: thread::JoinHandle<()> = net::lac_request_thread(
            conf.debug,
            thread_tx,
            thread_id,
            line_json["name"].as_str().unwrap().to_string()
        );

        // Push the new thread into the threads vector
        if threads.len() < n_threads {
            threads.push(thread);
        } else {
            threads[thread_id as usize] = thread;
        }

        // Total requests counter (printed every 500 request)
        stats.requests += 1;
        if stats.requests % 500 == 0 {
            println!("Requests: {}\n", stats.requests);
        }
    }

    // Wait for the end of all threads
    let mut id: u16 = 0;
    for thread in threads {
        thread.join().expect(&format!("The thread being joined has panicked. ID: {}\n", id));
        id += 1;
    }

    // Print results
    println!("{}", unindent(format!("
        === SCAN TEST COMPLETED ===
        
        Threads: {}
        Requests: {}
        Connection errors: {}
        Status != 200 OK: {}
        Https: {}
        Http: {}

        Matching services found: {}
        ===========================
    ",
        n_threads,
        stats.requests,
        stats.requests_errors,
        stats.requests_status_not_ok,
        stats.requests_https,
        stats.requests_http,
        stats.services_found).as_str())
    );
}
