use std::{
    env,
    fs::{
        self,
        File
    },
    path::Path,
    mem
};
use rusqlite;
use serde_json;
use rand::Rng;
use regex::Regex;
use semver::Version;
use crate::lachesis::{
    LacConf,
    Definition
};
use crate::db::DbMan;

pub fn get_cli_params() -> Result<LacConf, &'static str> {
    let mut conf = LacConf::default();

    let mut args = env::args();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--def" => {
                let file = match args.next() {
                    Some(arg) => arg,
                    None => return Err("Invalid value for parameter --def")
                };

                if Path::new(&format!("resources/definitions/{}.json", file)).exists() {
                    conf.definitions_paths
                        .push(format!("resources/definitions/{}.json", file));
                } else if Path::new(&format!("resources/definitions/{}", file)).exists() {
                    conf.definitions_paths
                        .push(format!("resources/definitions/{}", file));
                } else if Path::new(&file).exists() {
                    conf.definitions_paths.push(file);
                } else {
                    return Err("Invalid value for parameter --def (file not found)");
                }

                conf.definitions = match read_validate_definitions(&conf.definitions_paths) {
                    Ok(definitions) => definitions,
                    Err(err) => {
                        println!("{}", err);
                        return Err("Definitions validation failed");
                    }
                };
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
            "--ip-range" => {
                conf.ip_range.0 = match args.next() {
                    Some(arg) => {
                        if !is_valid_ipv4(&arg) {
                            return Err("Invalid value for parameter --ip-range (first ip)")
                        }
                        arg
                    }
                    None => return Err("Invalid value for parameter --ip-range (first ip)")
                };
                conf.ip_range.1 = match args.next() {
                    Some(arg) => {
                        if !is_valid_ipv4(&arg) {
                            return Err("Invalid value for parameter --ip-range (second ip)")
                        }
                        arg
                    }
                    None => return Err("Invalid value for parameter --ip-range (second ip)")
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
                conf.with_limit = true;
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
        if conf.dataset.is_empty() && conf.ip_range.0.is_empty() {
            return Err("One of the two parameters --dataset or --ip-range must be specified");
        }

        if !conf.dataset.is_empty() && !conf.ip_range.0.is_empty() {
            return Err("Parameters --dataset and --ip-range are mutually exclusive");
        }

        if !conf.ip_range.0.is_empty() {
            let ip_targets = count_ips_in_range(&conf.ip_range.0, &conf.ip_range.1).unwrap() as usize;
            if ip_targets < conf.max_targets {
                return Err("Parameter --max-target is less than the IPs of the specified range");
            }
        }

        if conf.definitions.is_empty() {
            let paths = fs::read_dir("resources/definitions").unwrap();

            for path in paths {
                conf.definitions_paths
                    .push(path.unwrap().path().to_str().unwrap().to_string());
            }

            if conf.definitions_paths.is_empty() {
                return Err("No definition files found in resources/definitions");
            } else {
                conf.definitions = match read_validate_definitions(&conf.definitions_paths) {
                    Ok(definitions) => definitions,
                    Err(err) => {
                        println!("{}", err);
                        return Err("Definitions validation failed");
                    }
                };
            }
        }
    }

    if conf.threads as usize > conf.max_targets {
        return Err("The number of threads can't be greater than the number of max targets");
    }

    Ok(conf)
}

pub fn read_validate_definitions(paths: &[String]) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(&path) {
            Ok(file) => file,
            Err(_err) => {
                return Err(format!("Definition file: {} not found.", path));
            }
        };

        // json structure validation
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

        // regexps and semver versions validation
        for def in &definitions_part {
            match Regex::new(def.service.regex.as_str()) {
                Ok(_re) => (),
                Err(err) => {
                    return Err(format!(
                        "Invalid regex: {} Error: {}\nDefinition file: {} Service: {}",
                        def.service.regex, err, path, def.name
                    ));
                }
            }

            if let Some(versions) = &def.versions {
                if let Some(semver) = &versions.semver {
                    match Regex::new(semver.regex.as_str()) {
                        Ok(_re) => (),
                        Err(err) => {
                            return Err(format!(
                                "Invalid regex: {} Error: {}\nDefinition file: {} Service: {}",
                                semver.regex, err, path, def.name
                            ));
                        }
                    }

                    for range in &semver.ranges {
                        match Version::parse(&range.from) {
                            Ok(_) => {}
                            Err(err) => {
                                return Err(format!(
                                    "Invalid semver: {} Error: {}\nDefinition file: {} Service: {}",
                                    range.from, err, path, def.name
                                ));
                            }
                        }
                        match Version::parse(&range.to) {
                            Ok(_) => {}
                            Err(err) => {
                                return Err(format!(
                                    "Invalid semver: {} Error: {}\nDefinition file: {} Service: {}",
                                    range.to, err, path, def.name
                                ));
                            }
                        }
                    }
                }

                if let Some(regex) = &versions.regex {
                    for r in regex {
                        match Regex::new(r.regex.as_str()) {
                            Ok(_re) => (),
                            Err(err) => {
                                return Err(format!(
                                    "Invalid regex: {} Error: {}\nDefinition file: {} Service: {}",
                                    r.regex, err, path, def.name
                                ));
                            }
                        }
                    }
                }
            }

            if def.protocol.as_str() == "tcp/custom" && def.options.message.is_none() {
                return Err(format!(
                    "Missing mandatory option 'message' for protocol tcp/custom. Definition file: {} Service: {}",
                    path, def.name
                ));
            }
        }
    }

    Ok(definitions)
}

pub fn print_records() -> Result<(), rusqlite::Error> {
    let dbm = DbMan::init()?;

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

fn is_valid_ipv4(ip: &str) -> bool {
    let ip1_parts = ip
        .split('.')
        .collect::<Vec<&str>>();

    if ip1_parts.len() != 4 {
        return false
    }

    for part in ip1_parts {
        let n = match part.parse::<u16>() {
            Ok(n) => n,
            Err(_err) => return false
        };
        if n > 255 { return false }
    }

    true
}

fn ip_to_u32(ip: &str) -> Result<u32, String> {
    let parts = ip
        .split('.')
        .collect::<Vec<&str>>();

    let mut n: u32 = 0;

    for (idx, p) in parts.into_iter().enumerate() {
        let p = match p.parse::<u32>() {
            Ok(n) => n,
            Err(_err) => return Err(format!("Invalid ip: {}", ip))
        };
        match idx {
            3 => n += p,
            2 => n += p * 256,        // 2^8
            1 => n += p * 65536,      // 2^16
            0 => n += p * 16_777_216, // 2^24
            _ => return Err(format!("Invalid ip: {}", ip))
        }
    }

    Ok(n)
}

fn count_ips_in_range(ip1: &str, ip2: &str) -> Result<u32, String> {
    let mut ip1 = ip_to_u32(ip1)?;
    let mut ip2 = ip_to_u32(ip2)?;

    if ip1 > ip2 {
        mem::swap(&mut ip1, &mut ip2)
    }

    Ok(ip2 - ip1)
}

pub fn random_ip_in_range(ip1: &str, ip2: &str) -> Result<String, String> {
    let mut random_ip = String::new();

    let ip1_parts = ip1.split('.');
    let mut ip1_parts_n: Vec<u16> = Vec::new();
    for part in ip1_parts {
        let n = match part.parse::<u16>() {
            Ok(n) => n,
            Err(_err) => return Err(format!("Invalid ip: {}", ip1))
        };
        ip1_parts_n.push(n);
    }

    let ip2_parts = ip2.split('.');
    let mut ip2_parts_n: Vec<u16> = Vec::new();
    for part in ip2_parts {
        let n = match part.parse::<u16>() {
            Ok(n) => n,
            Err(_err) => return Err(format!("Invalid ip: {}", ip2))
        };
        ip2_parts_n.push(n);
    }

    for i in 0..4 {
        if ip1_parts_n[i] > ip2_parts_n[i] {
            mem::swap(&mut ip1_parts_n[i], &mut ip2_parts_n[i])
        }
        let n = if ip1_parts_n[i] == ip2_parts_n[i] {
            ip1_parts_n[i]
        } else {
            rand::thread_rng().gen_range(ip1_parts_n[i], ip2_parts_n[i])
        };
        random_ip += &format!("{}", n);
        if i < 3 {
            random_ip += ".";
        }
    }

    Ok(random_ip)
}
