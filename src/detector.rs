use colored::Colorize;
use regex::Regex;
use semver::Version;

use crate::{conf::Definition, stats::format_host, worker::ReqTarget};

#[derive(Clone, Debug)]
pub struct DetectorResponse {
    pub target: ReqTarget,
    pub service: String,
    pub version: String,
    pub description: String,
    pub error: Option<String>,
}

impl DetectorResponse {
    fn default() -> DetectorResponse {
        DetectorResponse {
            target: ReqTarget::default(),
            service: String::new(),
            version: String::new(),
            description: String::new(),
            error: None,
        }
    }

    fn new(target: ReqTarget) -> Self {
        DetectorResponse {
            target,
            ..DetectorResponse::default()
        }
    }
}

pub fn detect(target: &ReqTarget, definitions: &[Definition]) -> Vec<DetectorResponse> {
    let mut matching = Vec::new();

    for def in definitions {
        let mut response = DetectorResponse::new(target.clone());

        let service_re = Regex::new(def.service.regex.as_str()).unwrap();
        match service_re.find(&target.response) {
            Some(m) => m,
            None => continue,
        };

        response.service = def.name.clone();
        if def.service.log {
            matching.push(response.clone());
        }

        let versions = match def.versions.clone() {
            Some(ver) => ver,
            None => continue,
        };

        if let Some(semver) = versions.semver {
            let version_re = Regex::new(semver.regex.as_str()).unwrap();
            let version_mat = match version_re.captures(&target.response) {
                Some(m) => m,
                None => continue,
            };

            response.version = version_mat["version"].to_string();

            // Incomplete semver fix (e.g. 4.6 -> 4.6.0)
            let mut dots = 0;
            for c in response.version.bytes() {
                if c == b'.' {
                    dots += 1;
                }
            }
            if dots < 2 {
                response.version += ".0";
            }

            let version = match Version::parse(response.version.as_str()) {
                Ok(ver) => ver,
                Err(_err) => {
                    response.error = Some(format!(
                        "[{}:{}] - Unknown or invalid semver: {}",
                        format_host(&response.target).cyan(),
                        target.port.to_string().cyan(),
                        response.version
                    ));
                    matching.push(response.clone());
                    continue;
                }
            };

            for ver in semver.ranges {
                if version >= Version::parse(ver.from.as_str()).unwrap()
                    && version <= Version::parse(ver.to.as_str()).unwrap()
                {
                    response.description = ver.description;
                    matching.push(response.clone());
                }
            }
        }

        if let Some(regex) = versions.regex {
            for ver in regex {
                let re = Regex::new(ver.regex.as_str()).unwrap();

                if let Some(_mat) = re.find(&target.response) {
                    response.version = ver.version;
                    response.description = ver.description;
                    matching.push(response.clone());
                }
            }
        }
    }

    matching
}
