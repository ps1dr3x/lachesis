use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
};

use clap::Parser;
use ipnet::{Ipv4AddrRange, Ipv4Net};
use serde_derive::{Deserialize, Serialize};
use tokio::sync::Mutex;
use validator::Validate;

use crate::validators::{
    validate_definition, validate_method, validate_path, validate_protocol, validate_regex,
    validate_regex_ver, validate_semver,
};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DbConf {
    pub host: String,
    pub port: String,
    pub dbname: String,
    pub user: String,
    pub password: String,
}

#[derive(Clone, Debug, Validate)]
pub struct Conf {
    pub db_conf: DbConf,
    #[validate(nested)]
    pub definitions: Vec<Definition>,
    pub dataset: String,
    pub subnets: Arc<Mutex<(Vec<Ipv4AddrRange>, usize)>>,
    pub user_agent: String,
    pub max_targets: u64,
    pub req_timeout: u64,
    pub max_concurrent_requests: usize,
    pub debug: bool,
    pub web_ui: bool,
    pub max_response_size: usize,
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
            max_response_size: 10240,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
#[validate(schema(function = validate_definition))]
pub struct Definition {
    pub name: String,
    #[validate(custom(function = validate_protocol))]
    pub protocol: String,
    pub options: Options,
    #[validate(nested)]
    pub service: Service,
    #[validate(nested)]
    pub versions: Option<Versions>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Options {
    #[validate(custom(function = validate_method))]
    pub method: Option<String>,
    #[validate(custom(function = validate_path))]
    pub path: Option<String>,
    pub headers: Option<Vec<(String, String)>>,
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub payload: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Service {
    #[validate(custom(function = validate_regex))]
    pub regex: String,
    pub log: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct Versions {
    #[validate(nested)]
    pub semver: Option<SemverVersions>,
    #[validate(custom(function = validate_regex_ver))]
    pub regex: Option<Vec<RegexVersion>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct SemverVersions {
    #[validate(custom(function = validate_regex))]
    pub regex: String,
    #[validate(nested)]
    pub ranges: Vec<RangeVersion>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RangeVersion {
    #[validate(custom(function = validate_semver))]
    pub from: String,
    #[validate(custom(function = validate_semver))]
    pub to: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Validate)]
pub struct RegexVersion {
    #[validate(custom(function = validate_regex))]
    pub regex: String,
    pub version: String,
    pub description: String,
}

pub fn parse_validate_definitions(paths: &[String]) -> Result<Vec<Definition>, String> {
    let mut definitions = Vec::new();

    for path in paths {
        let def_file = match File::open(path) {
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
    user_selected: Option<Vec<String>>,
    user_excluded: Option<Vec<String>>,
) -> Result<Vec<String>, &'static str> {
    match user_selected {
        Some(paths) => {
            let mut defs = Vec::new();

            for path in &paths {
                if Path::new(&format!("resources/definitions/{}.json", path)).exists() {
                    defs.push(format!("resources/definitions/{}.json", path));
                } else if Path::new(&format!("resources/definitions/{}", path)).exists() {
                    defs.push(format!("resources/definitions/{}", path));
                } else if Path::new(path).exists() {
                    defs.push(path.clone());
                } else {
                    return Err("Invalid value for parameter --def/-d (file not found)");
                }
            }

            Ok(defs)
        }
        None => {
            let mut defs = Vec::new();
            let excluded: Vec<String> = user_excluded.unwrap_or_default();

            let paths = fs::read_dir("resources/definitions").unwrap();
            for path in paths {
                let path = path.unwrap();
                let file_name = path.file_name();
                let file_name = file_name.to_str().unwrap().to_string();
                match file_name.find(".json") {
                    Some(idx) => {
                        if !excluded.contains(&file_name)
                            && !excluded.contains(&file_name[0..idx].to_string())
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

#[derive(Parser, Debug)]
#[command(
    name = "lachesis",
    version = "v0.4.0",
    author = "Michele Federici (@ps1dr3x) <michele@federici.tech>",
    about = "Web services mass scanner"
)]
struct Args {
    /// The full path of the DNS dataset used for the requests. JSONL, one record per line.
    /// An example of a compatible dataset is the forward DNS dataset by Rapid7 (https://opendata.rapid7.com/sonar.fdns_v2/).
    /// Example format of each line: {"name":"example.com","type":"a","value":"1.2.3.4"}
    #[arg(short = 'D', long, value_name = "FILE", conflicts_with_all = ["subnet", "web_ui"])]
    dataset: Option<String>,

    /// Scan one or more subnets (e.g. --subnet 10.0.0.0/24 --subnet 192.168.1.0/24)
    #[arg(short = 'S', long, value_name = "SUBNET", num_args = 1.., conflicts_with_all = ["dataset", "web_ui"])]
    subnet: Option<Vec<String>>,

    /// Definition file(s) to use. Default: all files in resources/definitions/
    #[arg(short = 'd', long, value_name = "FILE", num_args = 1..)]
    def: Option<Vec<String>>,

    /// Exclude specific definition file(s) (only when no --def is given)
    #[arg(short = 'e', long = "exclude-def", value_name = "FILE", num_args = 1.., conflicts_with = "def")]
    exclude_def: Option<Vec<String>>,

    /// Custom user agent string for HTTP/HTTPS requests
    #[arg(
        short = 'u',
        long = "user-agent",
        value_name = "STRING",
        default_value = "lachesis/0.4.0"
    )]
    user_agent: String,

    /// Maximum number of targets to scan
    #[arg(
        short = 'm',
        long = "max-targets",
        value_name = "NUM",
        conflicts_with = "web_ui"
    )]
    max_targets: Option<u64>,

    /// Maximum timeout per request in seconds
    #[arg(
        short = 't',
        long = "req-timeout",
        value_name = "NUM",
        default_value_t = 10
    )]
    req_timeout: u64,

    /// Maximum number of concurrent requests (0 = unlimited)
    #[arg(
        short = 'c',
        long = "max-concurrent-requests",
        value_name = "NUM",
        default_value_t = 0
    )]
    max_concurrent_requests: usize,

    /// Print debug messages
    #[arg(short = 'v', long)]
    debug: bool,

    /// Serve a web app (and a basic API) to visualize/explore collected data
    #[arg(short = 'w', long = "web-ui", conflicts_with_all = ["dataset", "subnet"])]
    web_ui: bool,

    /// Maximum response body size in bytes (HTTP and TCP)
    #[arg(
        short = 'r',
        long = "max-response-size",
        value_name = "BYTES",
        default_value_t = 10240
    )]
    max_response_size: usize,
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

    let args = Args::parse();

    // If --web-ui/-w option is specified, nothing else is needed
    if args.web_ui {
        return Ok(Conf {
            web_ui: true,
            ..Default::default()
        });
    }

    // Validate that at least one of --dataset or --subnet was provided
    if args.dataset.is_none() && args.subnet.is_none() {
        return Err("One of --dataset/-D or --subnet/-S is required");
    }

    // If a value for --dataset/-D is specified, check that the file exists
    let dataset = if let Some(ref path) = args.dataset {
        if !Path::new(path).exists() {
            return Err("Invalid value for parameter --dataset/-D (file not found)");
        }
        path.clone()
    } else {
        String::new()
    };

    // Load definitions (selected ones or all the files in resources/definitions folder
    // minus the excluded ones)
    let definitions_paths = search_definitions(args.def, args.exclude_def)?;
    let definitions = match parse_validate_definitions(&definitions_paths) {
        Ok(definitions) => definitions,
        Err(err) => {
            println!("{}", err);
            return Err("Definitions validation failed");
        }
    };

    // Parse subnets (if specified)
    let subnets = match args.subnet {
        Some(subnets) => {
            let mut sn = Vec::new();

            for subnet in &subnets {
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
        user_agent: args.user_agent,
        max_targets: args.max_targets.unwrap_or(0),
        req_timeout: args.req_timeout,
        max_concurrent_requests: args.max_concurrent_requests,
        debug: args.debug,
        web_ui: false,
        max_response_size: args.max_response_size,
    })
}
