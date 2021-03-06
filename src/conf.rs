use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
};

use clap::{App, Values};
use ipnet::{Ipv4AddrRange, Ipv4Net};
use serde_derive::{Deserialize, Serialize};
use tokio::sync::Mutex;
use validator::Validate;

use crate::validators::{
    validate_definition, validate_method, validate_path, validate_protocol, validate_regex,
    validate_regex_ver, validate_semver,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DbConf {
    pub host: String,
    pub port: String,
    pub dbname: String,
    pub user: String,
    pub password: String,
}

impl Default for DbConf {
    fn default() -> DbConf {
        DbConf {
            host: String::new(),
            port: String::new(),
            dbname: String::new(),
            user: String::new(),
            password: String::new(),
        }
    }
}

#[derive(Clone, Debug, Validate)]
pub struct Conf {
    pub db_conf: DbConf,
    #[validate]
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
    pub user_agent: String,
    pub max_targets: u64,
    pub req_timeout: u64,
    pub max_concurrent_requests: usize,
    pub debug: bool,
    pub web_ui: bool,
}

impl Default for Conf {
    fn default() -> Conf {
        Conf {
            db_conf: DbConf::default(),
            definitions: Vec::new(),
            dataset: String::new(),
            subnets: Arc::new(Mutex::new((Vec::new(), 0))),
            user_agent: String::new(),
            max_targets: 0,
            req_timeout: 10,
            max_concurrent_requests: 0,
            debug: false,
            web_ui: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
#[validate(schema(function = "validate_definition"))]
pub struct Definition {
    pub name: String,
    #[validate(custom = "validate_protocol")]
    pub protocol: String,
    pub options: Options,
    #[validate]
    pub service: Service,
    #[validate]
    pub versions: Option<Versions>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Options {
    #[validate(custom = "validate_method")]
    pub method: Option<String>,
    #[validate(custom = "validate_path")]
    pub path: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub payload: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Service {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub log: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Versions {
    #[validate]
    pub semver: Option<SemverVersions>,
    #[validate(custom = "validate_regex_ver")]
    pub regex: Option<Vec<RegexVersion>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct SemverVersions {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    #[validate]
    pub ranges: Vec<RangeVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RangeVersion {
    #[validate(custom = "validate_semver")]
    pub from: String,
    #[validate(custom = "validate_semver")]
    pub to: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RegexVersion {
    #[validate(custom = "validate_regex")]
    pub regex: String,
    pub version: String,
    pub description: String,
}

pub fn parse_validate_definitions(paths: &[String]) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(&path) {
            Ok(file) => file,
            Err(_err) => {
                return Err(format!(
                    "Definition file: {} not found or not readable.",
                    path
                ));
            }
        };

        // JSON typed parsing
        let definitions_part: Result<Vec<Definition>, serde_json::Error> =
            serde_json::from_reader(def_file);
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

fn search_definitions(
    user_selected: Option<Values>,
    user_excluded: Option<Values>,
) -> Result<Vec<String>, &'static str> {
    match user_selected {
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

            Ok(defs)
        }
        None => {
            let mut defs = Vec::new();
            let mut excluded = Vec::new();

            if let Some(edefs) = user_excluded {
                for edef in edefs {
                    excluded.push(edef);
                }
            };

            let paths = fs::read_dir("resources/definitions").unwrap();
            for path in paths {
                let path = path.unwrap();
                let file_name = path.file_name();
                let file_name = file_name.to_str().unwrap();
                match file_name.find(".json") {
                    Some(idx) => {
                        if !excluded.contains(&file_name) && !excluded.contains(&&file_name[0..idx])
                        {
                            defs.push(path.path().to_str().unwrap().to_string());
                        }
                    }
                    None => {
                        return Err("Found extraneous files in resources/definitions (not .json)")
                    }
                }
            }

            if defs.is_empty() {
                return Err("No definition files found in resources/definitions");
            }

            Ok(defs)
        }
    }
}

pub fn load_db_conf() -> Result<DbConf, &'static str> {
    let file = match File::open("conf/db-conf.json") {
        Ok(f) => f,
        Err(_) => {
            return Err("The Db conf file conf/db-conf.json doesn't exist or is not readable")
        }
    };

    match serde_json::from_reader(file) {
        Ok(db_conf) => Ok(db_conf),
        Err(_) => Err("The Db conf file conf/db-conf.json is invalid (json parse error)"),
    }
}

pub fn load() -> Result<Conf, &'static str> {
    let db_conf = load_db_conf()?;

    // Get cli parameters according to the definition file
    let cli_yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(cli_yaml).get_matches();

    // If --web-ui/-w option is specified, nothing else is needed
    if matches.is_present("web_ui") {
        return Ok(Conf {
            web_ui: true,
            ..Default::default()
        });
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
        match value_t!(matches, "max_targets", u64) {
            Ok(n) => n,
            Err(_) => {
                return Err("Invalid value for parameter --max-targets/-m (not a valid number)");
            }
        }
    } else {
        0
    };

    // If a value for --req-timeout/-t is specified, check that it's a valid number
    let req_timeout = match value_t!(matches, "req_timeout", u64) {
        Ok(n) => n,
        Err(_) => {
            return Err("Invalid value for parameter --req-timeout/-t (not a valid number)");
        }
    };

    // If a value for --max-concurrent-requests/-c is specified, check that it's a valid number
    let max_concurrent_requests = match value_t!(matches, "max_concurrent_requests", usize) {
        Ok(n) => n,
        Err(_) => {
            return Err(
                "Invalid value for parameter --max-concurrent-requests/-c (not a valid number)",
            );
        }
    };

    // Load definitions (selected ones or all the files in resources/definitions folder
    // minus the excluded ones)
    let definitions_paths =
        search_definitions(matches.values_of("def"), matches.values_of("exclude_def"))?;
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
            let mut sn = Vec::new();

            for subnet in subnets {
                match subnet.parse::<Ipv4Net>() {
                    Ok(net) => {
                        sn.push(net.hosts());
                    }
                    Err(_) => return Err("Invalid value for parameter --subnet"),
                }
            }

            Arc::new(Mutex::new((sn, 0)))
        }
        None => Arc::new(Mutex::new((Vec::new(), 0))),
    };

    Ok(Conf {
        db_conf,
        definitions,
        dataset,
        subnets,
        user_agent: String::from(matches.value_of("user_agent").unwrap()),
        max_targets,
        req_timeout,
        max_concurrent_requests,
        debug: matches.is_present("debug"),
        web_ui: false,
    })
}
