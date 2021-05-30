use hyper::Uri;
use regex::Regex;
use semver::Version;
use validator::ValidationError;

use crate::conf::{Definition, RegexVersion};

pub fn validate_protocol(protocol: &str) -> Result<(), ValidationError> {
    match protocol {
        "http/s" | "tcp/custom" => Ok(()),
        _ => Err(ValidationError::new(
            "Invalid protocol. Available options: 'http/s', 'tcp/custom'",
        )),
    }
}

pub fn validate_method(method: &str) -> Result<(), ValidationError> {
    match method {
        "GET" | "POST" | "PUT" | "DELETE" | "HEAD" | "OPTIONS" | "CONNECT" | "PATCH" | "TRACE" => {
            Ok(())
        }
        _ => Err(ValidationError::new("Invalid HTTP method. Available options: 'GET', 'POST', 'PUT', 'DELETE', 'HEAD', 'OPTIONS', 'CONNECT', 'PATCH', 'TRACE'")),
    }
}

pub fn validate_path(path: &str) -> Result<(), ValidationError> {
    match path.parse::<Uri>() {
        Ok(_) => Ok(()),
        Err(_e) => Err(ValidationError::new("Invalid path")),
    }
}

pub fn validate_regex(regex: &str) -> Result<(), ValidationError> {
    match Regex::new(regex) {
        Ok(_re) => Ok(()),
        Err(_e) => Err(ValidationError::new("Invalid regex")),
    }
}

pub fn validate_regex_ver(rv: &[RegexVersion]) -> Result<(), ValidationError> {
    for re in rv {
        validate_regex(&re.regex)?;
    }
    Ok(())
}

pub fn validate_semver(semver: &str) -> Result<(), ValidationError> {
    match Version::parse(&semver) {
        Ok(_) => Ok(()),
        Err(_e) => Err(ValidationError::new("Invalid semver")),
    }
}

pub fn validate_definition(def: &Definition) -> Result<(), ValidationError> {
    if def.protocol.as_str() == "tcp/custom" {
        if def.options.payload.is_none() {
            return Err(ValidationError::new(
                "Missing mandatory option field 'payload' for protocol 'tcp/custom'",
            ));
        }

        if def.options.method.is_some() || def.options.path.is_some() {
            return Err(ValidationError::new(
                "Option fields 'method' and 'path' can't be used with protocol 'tcp/custom'",
            ));
        }
    }

    if def.protocol.as_str() == "http/s" {
        if def.options.method.is_none() {
            return Err(ValidationError::new(
                "Missing mandatory option field 'method' for protocol 'http/s'",
            ));
        }

        if def.options.path.is_none() {
            return Err(ValidationError::new(
                "Missing mandatory option field 'path' for protocol 'http/s'",
            ));
        }

        let method = def.options.method.clone().unwrap();
        if def.options.payload.is_some()
            && (method == "GET"
                || method == "HEAD"
                || method == "OPTIONS"
                || method == "CONNECT"
                || method == "TRACE")
        {
            return Err(ValidationError::new(
                "Requests using HTTP methods: 'GET', 'HEAD', 'OPTIONS', 'CONNECT', 'TRACE' can't include a payload.",
            ));
        }

        if def.options.payload.is_none()
            && (method == "POST" || method == "PUT" || method == "DELETE" || method == "PATCH")
        {
            return Err(ValidationError::new(
                "Requests using HTTP methods: 'POST', 'PUT', 'DELETE', 'PATCH' should have a payload. To send an empty body, use \"payload\": \"\"",
            ));
        }
    }

    Ok(())
}
