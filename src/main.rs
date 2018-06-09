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

fn main() {
    println!("{}", unindent("

        
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-' 
                              v0.1.0

    "));

    // Check parameters
    let conf = utils::get_cli_params();
    if conf.is_err() {
        println!("{}", conf.err().unwrap());
        println!("{}", unindent("

            Usage:

            lachesis --file dns.json

            Optional arguments:

            --threads NUM (default: 250)
            --max-requests NUM (default: 10000)
            --debug
            --print-records

        "));
        return;
    }
    let conf: LacConf = conf.unwrap();

    // --print-records is true. Print records and exit
    if conf.print_records {
        let dbm = db::DbMan::new();
        let vulns = dbm.get_all_vuln().unwrap();
        if vulns.is_empty() {
            println!("Db is empty\n");
            return;
        }
        println!("Records:\n");
        for vuln in vulns {
            println!("{:?}", vuln);
        }
        return;
    }

    // Results counters
    let mut requests: usize = 0;
    let mut requests_errors: usize = 0;
    let mut requests_https: usize = 0;
    let mut requests_http: usize = 0;
    let mut requests_status_not_ok: usize = 0;
    let mut wordpress_sites: usize = 0;
    let mut potentially_vulnerable: usize = 0;

    // Open dns records file and instantiate the reader
    let dns_records_file_path: &Path = Path::new(conf.file_path.as_str());
    let dns_records_file: File = File::open(dns_records_file_path).unwrap();
    let mut easy_reader: EasyReader = EasyReader::new(dns_records_file).unwrap();

    // Threads vector and communication channel
    let n_threads = conf.threads;
    let mut threads: Vec<thread::JoinHandle<()>> = Vec::with_capacity(n_threads);
    let (tx, rx): (mpsc::Sender<LacResponse>, mpsc::Receiver<LacResponse>) = mpsc::channel();

    let n_requests = conf.max_requests;
    while requests < n_requests {
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
            if wr.is_request_error { requests_errors += 1; }
            if wr.is_https { requests_https += 1; }
            if wr.is_http { requests_http += 1; }
            if wr.is_status_not_ok { requests_status_not_ok += 1; }

            if wr.is_wordpress { wordpress_sites += 1; }
            if wr.is_potentially_vulnerable { potentially_vulnerable += 1; }

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
        requests += 1;
        if requests % 500 == 0 {
            println!("Requests: {}\n", requests);
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

        WordPress sites found: {}

        Potentially vulnerable: {}
        ===========================
    ",
        n_threads,
        requests,
        requests_errors,
        requests_status_not_ok,
        requests_https,
        requests_http,
        wordpress_sites,
        potentially_vulnerable).as_str())
    );
}
