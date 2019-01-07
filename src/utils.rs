use std::{
    fs::{
        self,
        File
    },
    sync::{
        Arc,
        Mutex
    },
    path::Path
};
use rusqlite;
use serde_json;
use ipnet::Ipv4Net;
use validator::Validate;
use clap::App;
use crate::lachesis::{
    LacConf,
    Definition
};
use crate::db::DbMan;

pub fn get_conf() -> Result<LacConf, &'static str> {
    // Get cli parameters according to the definition file
    let cli_yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(cli_yaml).get_matches();

    // If --print-records/-p option is specified, nothing else is needed
    if matches.is_present("print_records") {
        let mut conf = LacConf::default();
        conf.print_records = true;
        return Ok(conf);
    }

    // If a value for --dataset/-D is specified, check that the file exists
    let dataset = if matches.is_present("dataset") {
        let dataset = matches.value_of("dataset").unwrap();
        if !Path::new(dataset).exists() {
            return Err("Invalid value for parameter --dataset/-D (file not found)");
        }
        dataset.to_string()
    } else {
        String::new()
    };

    // If a value for --max-targets/-m is specified, check that it's a valid number
    let max_targets = if matches.is_present("max_targets") {
        match value_t!(matches, "max_targets", usize) {
            Ok(n) => n,
            Err(_e) => {
                return Err("Invalid value for parameter --max-targets/-m (not a valid number)");
            }
        }
    } else {
        0
    };

    // Load definitions (selected ones or all the files in resources/definitions folder)
    let definitions_paths = match matches.values_of("def") {
        Some(paths) => {
            let mut defs = Vec::new();

            for path in paths {
                if Path::new(&format!("resources/definitions/{}.json", path)).exists() {
                    defs.push(format!("resources/definitions/{}.json", path));
                } else if Path::new(&format!("resources/definitions/{}", path)).exists() {
                    defs.push(format!("resources/definitions/{}", path));
                } else if Path::new(&path).exists() {
                    defs.push(String::from(path));
                } else {
                    return Err("Invalid value for parameter --def/-d (file not found)");
                }
            }

            defs
        },
        None => {
            let mut defs = Vec::new();

            let paths = fs::read_dir("resources/definitions").unwrap();
            for path in paths {
                defs.push(path.unwrap().path().to_str().unwrap().to_string());
            }

            if defs.is_empty() {
                return Err("No definition files found in resources/definitions");
            }

            defs
        }
    };
    let definitions = match parse_validate_definitions(&definitions_paths) {
        Ok(definitions) => definitions,
        Err(err) => {
            println!("{}", err);
            return Err("Definitions validation failed");
        }
    };

    // Parse subnets (if specified)
    let subnets = match matches.values_of("subnet") {
        Some(subnets) => {
            let sn = Arc::new(Mutex::new((Vec::new(), 0)));

            for subnet in subnets {
                match subnet.parse::<Ipv4Net>() {
                    Ok(net) => {
                        sn.lock().unwrap().0.push(net.hosts());
                    },
                    Err(_err) => return Err("Invalid value for parameter --subnet")
                }
            }

            sn
        },
        None => {
            Arc::new(Mutex::new((Vec::new(), 0)))
        }
    };

    Ok(LacConf {
        definitions,
        dataset,
        subnets,
        debug: matches.is_present("debug"),
        max_targets,
        print_records: matches.is_present("print_records")
    })
}

pub fn parse_validate_definitions(paths: &[String]) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(&path) {
            Ok(file) => file,
            Err(_err) => {
                return Err(format!("Definition file: {} not found.", path));
            }
        };

        // JSON typed parsing
        let definitions_part: Result<Vec<Definition>, serde_json::Error> = serde_json::from_reader(def_file);
        let definitions_part = match definitions_part {
            Ok(definitions_part) => definitions_part,
            Err(err) => {
                return Err(format!(
                    "Definition file: {} JSON parsing error: {}",
                    path, err
                ))
            }
        };

        definitions.extend_from_slice(&definitions_part);

        // Fields validation
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
