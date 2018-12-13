use regex::Regex;
use semver::Version;
use colored::Colorize;
use crate::lachesis::Definition;

#[derive(Clone, Debug)]
pub struct DetectorResponse {
    pub service: String,
    pub version: String,
    pub description: String,
    pub host: String,
    pub port: u16,
    pub error: Option<String>
}

impl DetectorResponse {
    fn default() -> DetectorResponse {
        DetectorResponse {
            service: String::new(),
            version: String::new(),
            description: String::new(),
            host: String::new(),
            port: 0,
            error: None
        }
    }

    fn new(host: String, port: u16) -> Self {
        DetectorResponse {
            host,
            port,
            ..DetectorResponse::default()
        }
    }
}

pub fn detect(
        host: &str,
        port: u16,
        res_body: &str,
        definitions: &Vec<Definition>
    ) -> Vec<DetectorResponse> {

    let mut matching = Vec::new();

    for def in definitions {
        let mut response = DetectorResponse::new(String::from(host), port);

        let re = Regex::new(def.service.regex.as_str()).unwrap();

        let mat = match re.find(res_body) {
            Some(mat) => mat,
            None => continue
        };

        response.service = def.name.clone();
        if def.service.log {
            matching.push(response.clone());
        }

        let versions = match def.versions.clone() {
            Some(ver) => ver,
            None => continue
        };

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

            let version = match Version::parse(response.version.as_str()) {
                Ok(ver) => ver,
                Err(_err) => {
                    response.error = Some(format!(
                        "[{}][{}:{}] - Unknown or invalid semver: {}",
                        "WARN".yellow(), host, port, response.version
                    ));
                    matching.push(response.clone());
                    continue;
                }
            };

            for ver in semver.ranges {
                if version >= Version::parse(ver.from.as_str()).unwrap()
                    && version <= Version::parse(ver.to.as_str()).unwrap() {
                    response.description = ver.description;
                    matching.push(response.clone());
                }
            }
        }

        if let Some(regex) = versions.regex {
            for ver in regex {
                let re = Regex::new(ver.regex.as_str()).unwrap();

                if let Some(_mat) = re.find(res_body) {
                    response.version = ver.version;
                    response.description = ver.description;
                    matching.push(response.clone());
                }
            }
        }
    }

    matching
}
