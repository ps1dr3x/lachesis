use std::fs::File;

pub struct LacConf {
    pub file_path: String,
    pub debug: bool,
    pub help: bool,
    pub threads: u16,
    pub max_targets: usize,
    pub print_records: bool
}

pub fn get_cli_params() -> Result<LacConf, String> {
    use std::env;

    let mut conf = LacConf {
        file_path: "".to_string(),
        debug: false,
        help: false,
        threads: 250,
        max_targets: 10000,
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
            "--help" => {
                conf.help = true;
            },
            "--threads" => {
                let arg = args.next();
                if arg.is_none() {
                    return Err("Invalid value for parameter --threads".to_string());
                } else {
                    let threads = arg.unwrap().parse::<u16>();
                    if threads.is_err() {
                        return Err("Invalid value for parameter --threads".to_string());
                    }
                    conf.threads = threads.unwrap();
                }
            },
            "--max-targets" => {
                let arg = args.next();
                if arg.is_none() {
                    return Err("Invalid value for parameter --max-targets".to_string());
                } else {
                    let max_targets = arg.unwrap().parse::<usize>();
                    if max_targets.is_err() {
                        return Err("Invalid value for parameter --max-targets".to_string());
                    }
                    conf.max_targets = max_targets.unwrap();
                }
            },
            "--print-records" => {
                conf.print_records = true;
            },
            _ => {}
        }
    }

    if conf.file_path.is_empty() && !conf.help && !conf.print_records {
        return Err("Parameter --file is mandatory".to_string());
    }

    Ok(conf)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub protocol: String,
    pub options: Option<Options>,
    pub service: Service,
    pub versions: Option<Versions>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub ports: Vec<u16>,
    pub timeout: Option<bool>,
    pub message: Option<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub regex: String,
    pub log: bool
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Versions {
    pub semver: Option<SemverVersions>,
    pub regex: Option<Vec<RegexVersion>>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemverVersions {
    pub regex: String,
    pub ranges: Vec<RangeVersion>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RangeVersion {
    pub from: String,
    pub to: String,
    pub description: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegexVersion {
    pub regex: String,
    pub version: String,
    pub description: String
}

pub fn read_definitions() -> Result<Vec<Definition>, String> {
    use serde_json::{ from_reader, Error };

    let def_file = File::open("resources/definitions.json");
    if def_file.is_err() {
        return Err("Where is resources/definitions.json? :(".to_string());
    }
    let def_file = def_file.unwrap();

    let definitions: Result<Vec<Definition>, Error> = from_reader(def_file);
    match definitions {
        Ok(definitions) => Ok(definitions),
        Err(err) => Err(format!("JSON parser error: {}", err))
    }
}

pub struct Stats {
    pub targets: usize,
    pub requests: usize,
    pub unreachables: usize,
    pub requests_https: usize,
    pub requests_http: usize,
    pub requests_tcp_custom: usize,
    pub services_found: usize
}

impl Stats {
    pub fn default() -> Stats {
        Stats {
            targets: 0,
            requests: 0,
            unreachables: 0,
            requests_https: 0,
            requests_http: 0,
            requests_tcp_custom: 0,
            services_found: 0
        }
    }

    pub fn increment(&mut self, unreachable: bool, protocol: String, matching: bool) {
        if unreachable {
            self.unreachables += 1
        }

        match protocol.as_str() {
            "https" => {
                self.requests_https += 1;
            },
            "http" => {
                self.requests_http += 1;
            },
            "tcp/custom" => {
                self.requests_tcp_custom += 1;
            }
            _ => ()
        }

        if matching { self.services_found += 1; }
    }
}
