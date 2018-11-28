extern crate indicatif;

use unindent::unindent;
use self::indicatif::{ ProgressBar, ProgressStyle };

pub struct Stats {
    debug: bool,
    threads: u16,
    progress_bar: ProgressBar,
    targets: usize,
    unreachables: usize,
    requests_https: usize,
    requests_http: usize,
    requests_tcp_custom: usize,
    total_requests: usize,
    services_found: usize
}

impl Stats {
    pub fn new(threads: u16, max_targets: usize, debug: bool) -> Stats {
        let pb = ProgressBar::new(max_targets as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta})")
            .progress_chars("#>-"));

        Stats {
            debug: debug,
            threads: threads,
            progress_bar: pb,
            targets: 0,
            unreachables: 0,
            requests_https: 0,
            requests_http: 0,
            requests_tcp_custom: 0,
            total_requests: 0,
            services_found: 0
        }
    }

    pub fn increment(&mut self, target: bool, unreachable: bool, protocol: String, matching: bool) {
        if target {
            self.progress_bar.set_position(self.targets as u64);
            self.targets += 1;
            return;
        }

        if unreachable {
            self.unreachables += 1;
            return;
        }

        self.total_requests += 1;

        match protocol.as_str() {
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
        self.progress_bar.println(unindent(format!("

            ===== SCAN  COMPLETED =====
            
            Threads: {}
            Targets: {}
            Unreachables: {}
            Https: {}
            Http: {}
            Tcp/custom: {}
            Total successfull requests: {}

            Matching services found: {}

            ===========================
        ",
            self.threads,
            self.targets,
            self.unreachables,
            self.requests_https,
            self.requests_http,
            self.requests_tcp_custom,
            self.total_requests,
            self.services_found).as_str()
        ));
        self.progress_bar.finish();
    }
}
