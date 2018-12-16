use std::{
    env,
    fs::{
        self,
        File
    },
    path::Path
};
use rusqlite;
use serde_json;
use regex::Regex;
use semver::Version;
use ipnet::Ipv4Net;
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
            "--subnet" => {
                let arg = match args.next() {
                    Some(arg) => arg,
                    None => return Err("Missing value for parameter --subnet")
                };

                match arg.parse::<Ipv4Net>() {
                    Ok(net) => {
                        conf.subnets.lock().unwrap().push(net.hosts());
                    },
                    Err(_err) => return Err("Invalid value for parameter --subnet")
                }
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
        if conf.dataset.is_empty() && conf.subnets.lock().unwrap().is_empty() {
            return Err("One of the two parameters --dataset or --subnet must be specified");
        }

        if !conf.dataset.is_empty() && !conf.subnets.lock().unwrap().is_empty() {
            return Err("Parameters --dataset and --subnet are mutually exclusive");
        }

        if conf.subnets.lock().unwrap().len() > 1 {
            return Err("The --subnet parameter has been specified more than once. Multiple subnets are not yet supported");
        }

        if conf.definitions_paths.is_empty() {
            let paths = fs::read_dir("resources/definitions").unwrap();

            for path in paths {
                conf.definitions_paths
                    .push(path.unwrap().path().to_str().unwrap().to_string());
            }

            if conf.definitions_paths.is_empty() {
                return Err("No definition files found in resources/definitions");
            }
        }

        conf.definitions = match read_validate_definitions(&conf.definitions_paths) {
            Ok(definitions) => definitions,
            Err(err) => {
                println!("{}", err);
                return Err("Definitions validation failed");
            }
        };
    }

    if conf.max_targets != 0 && conf.threads as usize > conf.max_targets {
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
