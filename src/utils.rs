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
use validator::{
    Validate,
    ValidationError
};
use crate::lachesis::{
    LacConf,
    Definition,
    RegexVersion
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
                        conf.subnets.lock().unwrap().0.push(net.hosts());
                    },
                    Err(_err) => return Err("Invalid value for parameter --subnet")
                }
            }
            "--debug" => conf.debug = true,
            "--help" => conf.help = true,
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
        if conf.dataset.is_empty() && conf.subnets.lock().unwrap().0.is_empty() {
            return Err("One of the two parameters --dataset or --subnet must be specified");
        }

        if !conf.dataset.is_empty() && !conf.subnets.lock().unwrap().0.is_empty() {
            return Err("Parameters --dataset and --subnet are mutually exclusive");
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

    Ok(conf)
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

pub fn read_validate_definitions(paths: &[String]) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(&path) {
            Ok(file) => file,
            Err(_err) => {
                return Err(format!("Definition file: {} not found.", path));
            }
        };

        // Json parsing/structure validation
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

        // Definitions fields validation
        for def in &definitions_part {
            match def.validate() {
                Ok(_) => (),
                Err(err) => {
                    return Err(format!(
                        "Invalid definition: {} ({})\nError: {}",
                        def.name, path, err
                    ));
                }
            };
        }
    }

    Ok(definitions)
}

pub fn validate_protocol(protocol: &String) -> Result<(), ValidationError> {
    match protocol.as_str() {
        "http/s" | "tcp/custom" => Ok(()),
        _ => Err(ValidationError::new("Invalid protocol"))
    }
}

pub fn validate_regex(regex: &String) -> Result<(), ValidationError> {
    match Regex::new(regex) {
        Ok(_re) => Ok(()),
        Err(_err) => Err(ValidationError::new("Invalid regex"))
    }
}

pub fn validate_regex_ver(rv: &Vec<RegexVersion>) -> Result<(), ValidationError> {
    for re in rv { validate_regex(&re.regex)?; }
    Ok(())
}

pub fn validate_semver(semver: &String) -> Result<(), ValidationError> {
    match Version::parse(&semver) {
        Ok(_) => Ok(()),
        Err(_err) => Err(ValidationError::new("Invalid semver"))
    }
}

pub fn validate_definition(def: &Definition) -> Result<(), ValidationError> {
    if def.protocol.as_str() == "tcp/custom" && def.options.message.is_none() {
        Err(ValidationError::new("Missing mandatory option 'message' for protocol tcp/custom"))
    } else {
        Ok(())
    }
}

