extern crate regex;
extern crate semver;

use super::{ utils, utils::Definition };
use self::regex::Regex;
use self::semver::Version;

#[derive(Clone, Debug)]
pub struct DetectorResponse {
    pub service: String,
    pub version: String,
    pub description: String,
    pub host: String,
    pub port: u16
}

pub struct Detector {
    definitions: Vec<Definition>,
    host: String,
    port: u16,
    res_body: String,
    pub response: Vec<DetectorResponse>
}

impl Default for Detector {
    fn default() -> Detector {
        Detector {
            definitions: utils::read_definitions().unwrap(),
            host: "".to_string(),
            port: 0,
            res_body: "".to_string(),
            response: Vec::new()
        }
    }
}

impl Detector {
    pub fn new() -> Detector {
        Detector {
            ..Detector::default()
        }
    }

    pub fn run(&mut self, host: String, port: u16, res_body: String) -> &mut Detector {
        self.host = host;
        self.port = port;
        self.res_body = res_body;
        self.detect();
        self
    }

    fn detect(&mut self) {
        for def in &self.definitions {
            let mut response =  DetectorResponse {
                service: "".to_string(),
                version: "".to_string(),
                description: "".to_string(),
                host: "".to_string(),
                port: 0
            };

            let re = Regex::new(def.service.regex.as_str()).unwrap();
            let mat = re.find(self.res_body.as_str());

            if mat.is_none() { continue; }
            let mat = mat.unwrap();

            response.service = def.name.clone();
            if def.service.log {
                self.response.push(response.clone());
            }

            if def.versions.is_none() {
                return;
            }
            let versions = def.versions.clone().unwrap();

            if let Some(semver) = versions.semver {
                let mut dots = 0;
                let tmp_substring = self.res_body.bytes().skip(mat.end());
                for (_i, c) in tmp_substring.enumerate() {
                    if c == b'"' { break; }
                    if c == b'.' { dots += 1; }
                    response.version += (c as char).to_string().as_str();
                }
                // semver fix (e.g. 4.6 -> 4.6.0)
                if dots < 2 {
                    response.version += ".0";
                }

                let parsed_ver = Version::parse(response.version.as_str());
                if parsed_ver.is_err() {
                    println!("[{}:{}] - Unknown or invalid semver: {}\n", self.host, self.port, response.version);
                    continue;
                }

                let version = parsed_ver.unwrap();
                for ver in semver.ranges {
                    if version >= Version::parse(ver.from.as_str()).unwrap() &&
                        version <= Version::parse(ver.to.as_str()).unwrap() {
                        response.description = ver.description;
                        self.response.push(response.clone());
                    }
                }
            }
            
            if let Some(regex) = versions.regex {
                for ver in regex {
                    let re = Regex::new(ver.regex.as_str()).unwrap();
                    let mat = re.find(self.res_body.as_str());

                    if mat.is_none() { continue; }

                    response.version = ver.version;
                    response.description = ver.description;
                    self.response.push(response.clone());
                }
            }
        }
    }
}