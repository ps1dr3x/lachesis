use validator::ValidationError;
use regex::Regex;
use semver::Version;
use crate::lachesis::{
    Definition,
    RegexVersion
};

pub fn validate_protocol(protocol: &String) -> Result<(), ValidationError> {
    match protocol.as_str() {
        "http/s" | "tcp/custom" => Ok(()),
        _ => Err(ValidationError::new("Invalid protocol"))
    }
}

pub fn validate_regex(regex: &String) -> Result<(), ValidationError> {
    match Regex::new(regex) {
        Ok(_re) => Ok(()),
        Err(_err) => Err(ValidationError::new("Invalid regex"))
    }
}

pub fn validate_regex_ver(rv: &Vec<RegexVersion>) -> Result<(), ValidationError> {
    for re in rv { validate_regex(&re.regex)?; }
    Ok(())
}

pub fn validate_semver(semver: &String) -> Result<(), ValidationError> {
    match Version::parse(&semver) {
        Ok(_) => Ok(()),
        Err(_err) => Err(ValidationError::new("Invalid semver"))
    }
}

pub fn validate_definition(def: &Definition) -> Result<(), ValidationError> {
    if def.protocol.as_str() == "tcp/custom" && def.options.message.is_none() {
        Err(ValidationError::new("Missing mandatory option 'message' for protocol tcp/custom"))
    } else {
        Ok(())
    }
}
