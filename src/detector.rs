extern crate serde_json;
extern crate regex;
extern crate semver;

use std::path::Path;
use self::regex::Regex;
use self::semver::Version;
use super::db::Vulnerable;

pub struct WPPlugins {
    pub test: bool
}

pub struct WordPress {
    pub is_wordpress: bool,
    pub version: String,
    pub is_outdated: bool,
    pub potentially_vulnerable_plugins: bool,
    pub plugins: WPPlugins
} 

pub struct DetectorResponse {
    pub potentially_vulnerable: bool,
    pub vulnerable: Vec<Vulnerable>,
    pub wordpress: WordPress
}

pub struct Detector {
    definitions: serde_json::Value,
    host: String,
    port: u16,
    res_body: String,
    pub response: DetectorResponse
}

impl Default for Detector {
    fn default() -> Detector {
        Detector {
            definitions: super::utils::read_json_file(Path::new("resources/definitions.json")),
            host: "".to_string(),
            port: 0,
            res_body: "".to_string(),
            response: DetectorResponse {
                potentially_vulnerable: false,
                vulnerable: Vec::new(),
                wordpress: WordPress {
                    is_wordpress: false,
                    version: "".to_string(),
                    is_outdated: false,
                    potentially_vulnerable_plugins: false,
                    plugins: WPPlugins {
                        test: false
                    }
                }
            }
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
        self.detect_wordpress();
        self
    }

    fn detect_wordpress(&mut self) {
        let re = Regex::new(r#"(?m)<meta name="generator" content="WordPress "#).unwrap();
        let mat = re.find(self.res_body.as_str());
        match mat {
            Some(m) => {
                self.response.wordpress.is_wordpress = true;

                let mut dots = 0;
                let tmp_substring = self.res_body.bytes().skip(m.end());
                for (_i, c) in tmp_substring.enumerate() {
                    if c == b'"' { break; }
                    if c == b'.' { dots += 1; }
                    self.response.wordpress.version += (c as char).to_string().as_str();
                }
                // semver fix (e.g. 4.6 -> 4.6.0)
                if dots < 2 {
                    self.response.wordpress.version += ".0";
                }

                let parsed_ver = Version::parse(self.response.wordpress.version.as_str());
                match parsed_ver {
                    Ok(version) => {
                        let wp_latest_version = self.definitions["wordpress"]["latest_version"].as_str().unwrap();
                        let wp_vulnerable_versions = self.definitions["wordpress"]["vulnerable_versions"].as_array().unwrap();

                        if version < Version::parse(wp_latest_version).unwrap() {
                            self.response.wordpress.is_outdated = true;
                        }

                        for vuln in wp_vulnerable_versions {
                            if version >= Version::parse(vuln["from"].as_str().unwrap()).unwrap() &&
                                version <= Version::parse(vuln["to"].as_str().unwrap()).unwrap() {
                                self.response.potentially_vulnerable = true;

                                self.response.vulnerable.push(Vulnerable {
                                    service: "wordpress".to_string(),
                                    version: self.response.wordpress.version.clone(),
                                    exploit: vuln["exploit"].as_str().unwrap().to_string(),
                                    host: self.host.clone(),
                                    port: self.port
                                });
                            }
                        }
                    },
                    Err(_e) => {
                        println!("Unknown WordPress version: {}\n", self.response.wordpress.version);
                    }
                }
            },
            None => {}
        }
    }
}