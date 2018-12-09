use std::{
    fs::{
        self,
        File
    },
    path::Path,
    mem
};
use rusqlite;
use serde_json;
use crate::lachesis::{
    LacConf,
    Definition
};
use crate::db::DbMan;

pub fn get_cli_params() -> Result<LacConf, &'static str> {
    use std::env;

    let mut conf = LacConf {
        definitions: Vec::new(),
        dataset: "".to_string(),
        debug: false,
        help: false,
        threads: 4,
        max_targets: 1000,
        print_records: false
    };

    let mut args = env::args();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--def" => {
                let file = match args.next() {
                    Some(arg) => arg,
                    None => return Err("Invalid value for parameter --def")
                };

                if Path::new(&format!("resources/definitions/{}.json", file)).exists() {
                    conf.definitions
                        .push(format!("resources/definitions/{}.json", file));
                } else if Path::new(&format!("resources/definitions/{}", file)).exists() {
                    conf.definitions
                        .push(format!("resources/definitions/{}", file));
                } else if Path::new(&file).exists() {
                    conf.definitions.push(file);
                } else {
                    return Err("Invalid value for parameter --def (file not found)");
                }
            }
            "--dataset" => {
                conf.dataset = match args.next() {
                    Some(arg) => {
                        if !Path::new(&arg).exists() {
                            return Err("Invalid value for parameter --dataset (file not found)");
                        }
                        arg
                    }
                    None => return Err("Invalid value for parameter --dataset")
                };
            }
            "--debug" => conf.debug = true,
            "--help" => conf.help = true,
            "--threads" => {
                conf.threads = match args.next() {
                    Some(arg) => match arg.parse::<u16>() {
                        Ok(threads) => threads,
                        Err(_err) => return Err("Invalid value for parameter --threads")
                    },
                    None => return Err("Invalid value for parameter --threads")
                };
            }
            "--max-targets" => {
                conf.max_targets = match args.next() {
                    Some(arg) => match arg.parse::<usize>() {
                        Ok(max_targets) => max_targets,
                        Err(_err) => return Err("Invalid value for parameter --max-targets")
                    },
                    None => return Err("Invalid value for parameter --max-targets")
                };
            }
            "--print-records" => {
                conf.print_records = true;
            }
            _ => {}
        }
    }

    if !conf.help && !conf.print_records {
        if conf.dataset.is_empty() {
            return Err("Parameter --dataset is mandatory");
        }

        if conf.definitions.is_empty() {
            let paths = fs::read_dir("resources/definitions").unwrap();

            for path in paths {
                conf.definitions
                    .push(path.unwrap().path().to_str().unwrap().to_string());
            }

            if conf.definitions.is_empty() {
                return Err("No definition files found in resources/definitions");
            }
        }
    }

    if conf.threads as usize > conf.max_targets {
        return Err("The number of threads can't be greater than the number of max targets");
    }

    Ok(conf)
}

pub fn read_validate_definitions(paths: Vec<String>) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(&path) {
            Ok(file) => file,
            Err(_err) => {
                return Err(format!("Definition file: {} not found.", path));
            }
        };

        let definitions_part: Result<Vec<Definition>, serde_json::Error> = serde_json::from_reader(def_file);
        let definitions_part = match definitions_part {
            Ok(definitions_part) => definitions_part,
            Err(err) => {
                return Err(format!(
                    "Definition file: {} JSON parser error: {}",
                    path, err
                ))
            }
        };

        definitions.extend_from_slice(&definitions_part);

        for def in &definitions_part {
            if def.protocol.as_str() != "tcp/custom" { continue; }
            if def.options.message.is_none() {
                return Err(format!(
                    "Missing mandatory option 'message' for protocol tcp/custom. Definition file: {} Service: {}",
                    path,
                    def.name
                ));
            }
        }
    }

    Ok(definitions)
}

pub fn print_records() -> Result<(), rusqlite::Error> {
    let dbm = DbMan::new()?;

    let records = dbm.get_all_services()?;
    if records.is_empty() {
        println!("Db is empty or not exists yet\n");
        return Ok(());
    }
    println!("{} records:\n", records.len());
    for rec in records {
        println!("{:?}", rec);
    }

    Ok(())
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
            0 => n += p * 16_777_216, // 2^24
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
        mem::swap(&mut hex, &mut hex2)
    }

    let mut i: u32 = hex;
    while i <= hex2 {
        println!(
            "{}",
            format!(
                "{}.{}.{}.{}",
                i >> 24 & 0xff,
                i >> 16 & 0xff,
                i >> 8 & 0xff,
                i & 0xff
            )
        );
        i += 1
    }
}
