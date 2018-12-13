use indicatif::{
    ProgressBar,
    ProgressStyle
};
use colored::Colorize;

pub struct Stats {
    debug: bool,
    progress_bar: ProgressBar,
    with_limit: bool,
    targets: usize,
    requests_https: usize,
    requests_http: usize,
    requests_tcp_custom: usize,
    total_requests: usize,
    services_found: usize
}

impl Stats {
    pub fn new(with_limit: bool, max_targets: usize, debug: bool) -> Self {
        let pb = if with_limit {
            let pb = ProgressBar::new(max_targets as u64);
            pb.set_style(ProgressStyle::default_bar()
                .template("\n{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta})\n  {wide_msg}")
                .progress_chars("#>-"));
            pb
        } else {
            let pb = ProgressBar::new(0);
            pb.set_style(ProgressStyle::default_spinner()
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
                .template("\n{prefix:.bold.dim} {spinner} {wide_msg}"));
            pb
        };

        Stats {
            debug,
            progress_bar: pb,
            with_limit,
            targets: 0,
            requests_https: 0,
            requests_http: 0,
            requests_tcp_custom: 0,
            total_requests: 0,
            services_found: 0
        }
    }

    pub fn increment(&mut self, target: bool, protocol: &str, matching: bool) {
        if target {
            if self.with_limit {
                self.progress_bar.set_position(self.targets as u64);
            }

            self.progress_bar.set_message(
                &format!(
                    "Targets {}   Https {}   Http {}   Tcp/custom {}   Matching {}",
                    self.targets.to_string().cyan(),
                    self.requests_https.to_string().cyan(),
                    self.requests_http.to_string().cyan(),
                    self.requests_tcp_custom.to_string().cyan(),
                    self.services_found.to_string().cyan()
                )
            );

            self.targets += 1;
            return;
        }

        self.total_requests += 1;

        match protocol {
            "https" => self.requests_https += 1,
            "http" => self.requests_http += 1,
            "tcp/custom" => self.requests_tcp_custom += 1,
            _ => ()
        }

        if matching { self.services_found += 1; }
    }

    pub fn log(&mut self, message: String) {
        self.progress_bar.println(message);
    }

    pub fn log_debug(&mut self, message: String) {
        if self.debug {
            self.progress_bar.println(message);
        }
    }

    pub fn finish(&mut self) {
        self.progress_bar.finish();
    }
}
