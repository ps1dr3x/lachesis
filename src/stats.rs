use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use std::{thread, time::Instant};

use crate::detector::DetectorResponse;
use crate::utils;

struct RequestStatus {
    successful: u64,
    avg_time: u128,
    failed: u64,
    timedout: u64,
}

impl RequestStatus {
    fn default() -> Self {
        RequestStatus {
            successful: 0,
            avg_time: 0,
            failed: 0,
            timedout: 0,
        }
    }
    fn total(&self) -> u64 {
        self.successful + self.failed + self.timedout
    }
}

pub struct Stats {
    progress_bars: Vec<ProgressBar>,
    max_targets: u64,
    targets: u64,
    https: RequestStatus,
    http: RequestStatus,
    tcp_custom: RequestStatus,
    matching: u64,
}

impl Stats {
    pub fn new(max_targets: u64) -> Self {
        let m = MultiProgress::new();
        let mut pbs = Vec::new();
        let pb0 = if max_targets != 0 {
            let pb = ProgressBar::new(max_targets as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("\n[{elapsed_precise}] [{bar:40.cyan/blue}] ({eta})")
                    .progress_chars("#>-"),
            );
            pb
        } else {
            let pb = ProgressBar::new(0);
            pb.set_style(
                ProgressStyle::default_spinner().template("\n[{elapsed_precise}] {spinner:.green}"),
            );
            pb
        };

        let pb1 = ProgressBar::new(0);
        pb1.set_style(ProgressStyle::default_spinner().template("{wide_msg}"));
        let pb2 = ProgressBar::new(0);
        pb2.set_style(ProgressStyle::default_spinner().template("{wide_msg}"));
        let pb3 = ProgressBar::new(0);
        pb3.set_style(ProgressStyle::default_spinner().template("{wide_msg}"));
        let pb4 = ProgressBar::new(0);
        pb4.set_style(ProgressStyle::default_spinner().template("{wide_msg}"));
        pbs.push(m.add(pb0));
        pbs.push(m.add(pb1));
        pbs.push(m.add(pb2));
        pbs.push(m.add(pb3));
        pbs.push(m.add(pb4));

        thread::spawn(move || m.join().unwrap());

        Stats {
            progress_bars: pbs,
            max_targets,
            targets: 0,
            https: RequestStatus::default(),
            http: RequestStatus::default(),
            tcp_custom: RequestStatus::default(),
            matching: 0,
        }
    }

    fn total(&self) -> u64 {
        self.https.total() + self.http.total() + self.tcp_custom.total()
    }

    fn total_successful(&self) -> u64 {
        self.https.successful + self.http.successful + self.tcp_custom.successful
    }

    fn total_avg_time(&self) -> u128 {
        (self.https.avg_time + self.http.avg_time + self.tcp_custom.avg_time) / 3
    }

    fn total_failed(&self) -> u64 {
        self.https.failed + self.http.failed + self.tcp_custom.failed
    }

    fn total_timedout(&self) -> u64 {
        self.https.timedout + self.http.timedout + self.tcp_custom.timedout
    }

    pub fn increment_successful(&mut self, protocol: &str, matching: bool) {
        match protocol {
            "https" => self.https.successful += 1,
            "http" => self.http.successful += 1,
            "tcp/custom" => self.tcp_custom.successful += 1,
            _ => (),
        }

        if matching {
            self.matching += 1;
        }

        self.update_message();
    }

    pub fn increment_failed(&mut self, protocol: &str) {
        match protocol {
            "https" => self.https.failed += 1,
            "http" => self.http.failed += 1,
            "tcp/custom" => self.tcp_custom.failed += 1,
            _ => (),
        }

        self.update_message();
    }

    pub fn increment_timedout(&mut self, protocol: &str) {
        match protocol {
            "https" => self.https.timedout += 1,
            "http" => self.http.timedout += 1,
            "tcp/custom" => self.tcp_custom.timedout += 1,
            _ => (),
        }

        self.update_message();
    }

    pub fn increment_targets(&mut self) {
        if self.max_targets != 0 {
            self.progress_bars[0].set_position(self.targets as u64);
        }

        self.targets += 1;

        self.update_message();
    }

    pub fn update_avg_time(&mut self, time: Instant, protocol: &str) {
        match protocol {
            "https" => {
                self.https.avg_time = (self.https.avg_time * self.https.successful as u128
                    + time.elapsed().as_millis())
                    / (self.https.successful + 1) as u128
            }
            "http" => {
                self.http.avg_time = (self.http.avg_time * self.http.successful as u128
                    + time.elapsed().as_millis())
                    / (self.http.successful + 1) as u128
            }
            "tcp/custom" => {
                self.tcp_custom.avg_time = (self.tcp_custom.avg_time
                    * self.tcp_custom.successful as u128
                    + time.elapsed().as_millis())
                    / (self.tcp_custom.successful + 1) as u128
            }
            _ => (),
        };

        self.update_message();
    }

    fn update_message(&self) {
        self.progress_bars[1].set_message(&format!(
            "Targets: {} Requests [total: {} successful: {} avg_time: {}ms failed: {} timedout: {} matching: {}]",
            self.targets.to_string().cyan(),
            self.total().to_string().green(),
            self.total_successful().to_string().green(),
            self.total_avg_time().to_string().cyan(),
            self.total_failed().to_string().red(),
            self.total_timedout().to_string().yellow(),
            self.matching.to_string().cyan(),
        ));

        self.progress_bars[2].set_message(&format!(
            "Https [successful: {} avg_time: {}ms failed: {} timedout: {}]",
            self.https.successful.to_string().green(),
            self.https.avg_time.to_string().cyan(),
            self.https.failed.to_string().red(),
            self.https.timedout.to_string().yellow(),
        ));

        self.progress_bars[3].set_message(&format!(
            "Http [successful: {} avg_time: {}ms failed: {} timedout: {}]",
            self.http.successful.to_string().green(),
            self.http.avg_time.to_string().cyan(),
            self.http.failed.to_string().red(),
            self.http.timedout.to_string().yellow(),
        ));

        self.progress_bars[4].set_message(&format!(
            "Tcp/custom [successful: {} avg_time: {}ms failed: {} timedout: {}]",
            self.tcp_custom.successful.to_string().green(),
            self.tcp_custom.avg_time.to_string().cyan(),
            self.tcp_custom.failed.to_string().red(),
            self.tcp_custom.timedout.to_string().yellow(),
        ));
    }

    pub fn log_info(&mut self, message: String) {
        self.progress_bars[0].println(format!("[{}]{}", "INFO".yellow(), message));
    }

    pub fn log_err(&mut self, message: String) {
        self.progress_bars[0].println(format!("[{}]{}", "ERROR".red(), message));
    }

    pub fn log_match(&mut self, dr: &DetectorResponse) {
        self.progress_bars[0].println(format!(
            "[{}][{}] service: {} version: {} description: {}",
            "MATCH".green(),
            utils::format_host(&dr.target).green(),
            dr.service.green(),
            dr.version.green(),
            dr.description.green()
        ));
    }

    pub fn finish(&mut self) {
        if self.max_targets != 0 && self.targets < self.max_targets {
            self.log_info(format!(
                "All the targets have been consumed before reaching the specified max-targets number. targets: {} max_targets: {}",
                self.targets, self.max_targets
            ));
        }
        self.progress_bars[0].finish();
        self.progress_bars[1].finish();
        self.progress_bars[2].finish();
        self.progress_bars[3].finish();
        self.progress_bars[4].finish();
    }
}
