extern crate regex;
extern crate semver;

use self::regex::Regex;
use self::semver::Version;
use utils::Definition;

#[derive(Clone, Debug)]
pub struct DetectorResponse {
    pub service: String,
    pub version: String,
    pub description: String,
    pub host: String,
    pub port: u16
}

pub struct Detector {
    definitions: Vec<Definition>
}

impl Detector {
    pub fn new(definitions: Vec<Definition>) -> Detector {
        Detector {
            definitions: definitions
        }
    }

    pub fn run(&mut self, host: String, port: u16, res_body: String) -> Vec<DetectorResponse> {
        let mut matching = Vec::new();
        for def in &self.definitions {
            let mut response = DetectorResponse {
                service: "".to_string(),
                version: "".to_string(),
                description: "".to_string(),
                host: host.clone(),
                port: port
            };

            let re = Regex::new(def.service.regex.as_str()).unwrap();
            let mat = re.find(res_body.as_str());

            if mat.is_none() { continue; }
            let mat = mat.unwrap();

            response.service = def.name.clone();
            if def.service.log {
                matching.push(response.clone());
            }

            if def.versions.is_none() {
                continue;
            }
            let versions = def.versions.clone().unwrap();

            if let Some(semver) = versions.semver {
                let mut dots = 0;
                let tmp_substring = res_body.bytes().skip(mat.end());
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
                    println!("[{}:{}] - Unknown or invalid semver: {}", host, port, response.version);
                    continue;
                }

                let version = parsed_ver.unwrap();
                for ver in semver.ranges {
                    if version >= Version::parse(ver.from.as_str()).unwrap() &&
                        version <= Version::parse(ver.to.as_str()).unwrap() {
                        response.description = ver.description;
                        matching.push(response.clone());
                    }
                }
            }
            
            if let Some(regex) = versions.regex {
                for ver in regex {
                    let re = Regex::new(ver.regex.as_str()).unwrap();
                    let mat = re.find(res_body.as_str());

                    if mat.is_none() { continue; }

                    response.version = ver.version;
                    response.description = ver.description;
                    matching.push(response.clone());
                }
            }
        }
        matching
    }
}