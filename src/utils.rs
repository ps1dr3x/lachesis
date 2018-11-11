use std::fs::File;

pub struct LacConf {
    pub file_path: String,
    pub debug: bool,
    pub help: bool,
    pub threads: u16,
    pub max_targets: usize,
    pub print_records: bool
}

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub protocol: String,
    pub options: Options,
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
