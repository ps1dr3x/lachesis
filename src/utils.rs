use crate::worker::Target;

pub fn format_host(target: &Target) -> String {
    if !target.domain.is_empty() {
        format!("{} -> {}", target.ip, target.domain)
    } else {
        target.ip.clone()
    }
}
