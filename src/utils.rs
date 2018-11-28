use std::fs::File;
use lachesis::{
    LacConf,
    Definition
};
use db::DbMan;

pub fn get_cli_params() -> Result<LacConf, &'static str> {
    use std::env;

    let mut conf = LacConf {
        file_path: "".to_string(),
        debug: false,
        help: false,
        threads: 4,
        max_targets: 1000,
        print_records: false
    };

    let mut args = env::args();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => {
                conf.file_path = match args.next() {
                    Some(arg) => arg,
                    None => return Err("Invalid value for parameter --file")
                };
            },
            "--debug" => conf.debug = true,
            "--help" => conf.help = true,
            "--threads" => {
                conf.threads = match args.next() {
                    Some(arg) => {
                        match arg.parse::<u16>() {
                            Ok(threads) => threads,
                            Err(_err) => return Err("Invalid value for parameter --threads")
                        }
                    },
                    None => return Err("Invalid value for parameter --threads")
                };
            },
            "--max-targets" => {
                conf.max_targets = match args.next() {
                    Some(arg) => {
                        match arg.parse::<usize>() {
                            Ok(max_targets) => max_targets,
                            Err(_err) => return Err("Invalid value for parameter --max-targets")
                        }
                    },
                    None => return Err("Invalid value for parameter --max-targets")
                };
            },
            "--print-records" => {
                conf.print_records = true;
            },
            _ => {}
        }
    }

    if conf.file_path.is_empty() && !conf.help && !conf.print_records {
        return Err("Parameter --file is mandatory");
    }

    if conf.threads as usize > conf.max_targets {
        return Err("The number of threads can't be greater than the number of max targets");
    }

    Ok(conf)
}

pub fn read_validate_definitions() -> Result<Vec<Definition>, String> {
    use serde_json::{ from_reader, Error };

    let def_file = match File::open("resources/definitions.json") {
        Ok(file) => file,
        Err(_err) => {
            return Err("Where is resources/definitions.json? :(".to_string());
        }
    };

    let definitions: Result<Vec<Definition>, Error> = from_reader(def_file);
    let definitions = match definitions {
        Ok(definitions) => definitions,
        Err(err) => {
            return Err(format!("JSON parser error: {}", err))
        }
    };

    for def in definitions.clone() {
        if def.protocol.as_str() != "tcp/custom" { continue; }
        if def.options.message.is_none() {
            return Err(format!("Missing mandatory option 'message' for protocol tcp/custom. Service: {}", def.name));
        }
    }

    Ok(definitions)
}

pub fn print_records() {
    let dbm = DbMan::new();
    let records = dbm.get_all_services().unwrap();
    if records.is_empty() {
        println!("Db is empty or not exists yet\n");
        return;
    }
    println!("{} records:\n", records.len());
    for rec in records {
        println!("{:?}", rec);
    }
}

#[allow(dead_code)]
fn ip2hex(ip: &str) -> u32 {
    let parts = ip.split('.').map(|p| p.parse::<u32>().unwrap());

    let mut n: u32 = 0;

    for (idx, p) in parts.enumerate() {
        match idx {
            3 => n += p,
            2 => n += p * 256,        // 2^8
            1 => n += p * 65536,      // 2^16
            0 => n += p * 16777216,   // 2^24
            _ => println!("?"),
        }
    }

    n
}

#[allow(dead_code)]
pub fn ip_range(ip1: &str, ip2: &str) {
    let mut hex: u32 = ip2hex(ip1);
    let mut hex2: u32 = ip2hex(ip2);

    if hex > hex2 {
        let tmp = hex;
        hex = hex2;
        hex2 = tmp;
    }

    let mut i: u32 = hex;
    while i <= hex2 {
        println!("{}", format!("{}.{}.{}.{}", i >> 24 & 0xff, i >> 16 & 0xff, i >> 8 & 0xff, i & 0xff));
        i += 1
    }
}
