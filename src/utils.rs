extern crate serde_json;

use std::env;
use std::path::Path;
use std::fs::read_to_string;

pub struct LacConf {
    pub file_path: String,
    pub debug: bool,
    pub threads: usize,
    pub max_requests: usize,
    pub print_records: bool
}

pub fn get_cli_params() -> Result<LacConf, String> {
    let mut conf = LacConf {
        file_path: "".to_string(),
        debug: false,
        threads: 250,
        max_requests: 10000,
        print_records: false
    };

    let mut args = env::args();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => {
                let arg = args.next();
                if arg.is_none() {
                    return Err("Invalid value for parameter --file".to_string());
                } else {
                    conf.file_path = arg.unwrap();
                }
            },
            "--debug" => {
                conf.debug = true;
            },
            "--threads" => {
                let arg = args.next();
                if arg.is_none() {
                    return Err("Invalid value for parameter --threads".to_string());
                } else {
                    let threads = arg.unwrap().parse::<usize>();
                    if threads.is_err() {
                        return Err("Invalid value for parameter --threads".to_string());
                    }
                    conf.threads = threads.unwrap();
                }
            },
            "--max-requests" => {
                let arg = args.next();
                if arg.is_none() {
                    return Err("Invalid value for parameter --max-requests".to_string());
                } else {
                    let max_requests = arg.unwrap().parse::<usize>();
                    if max_requests.is_err() {
                        return Err("Invalid value for parameter --max-requests".to_string());
                    }
                    conf.max_requests = max_requests.unwrap();
                }
            },
            "--print-records" => {
                conf.print_records = true;
            },
            _ => {}
        }
    }

    if conf.file_path.is_empty() && !conf.print_records {
        return Err("Parameter --file is mandatory".to_string());
    }

    Ok(conf)
}

pub fn read_json_file<P: AsRef<Path>>(path: P) -> serde_json::Value {
    let content = read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}
