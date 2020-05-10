use std::{fs::File, io::prelude::Write, path::Path, thread, time};

use headless_chrome::{browser, protocol::page::ScreenshotFormat, Browser, LaunchOptionsBuilder};

use crate::worker::Target;

pub fn maybe_take_screenshot(target: &Target, id: String) {
    if target.protocol != "https" && target.protocol != "http" {
        return;
    }

    let target = target.clone();
    thread::spawn(move || {
        let browser_path = match browser::default_executable() {
            Ok(path) => path,
            Err(_) => return,
        };

        let browser_options = LaunchOptionsBuilder::default()
            .path(Some(browser_path))
            .build()
            .unwrap();
        let browser = match Browser::new(browser_options) {
            Ok(b) => b,
            Err(_) => return,
        };
        browser.wait_for_initial_tab().unwrap();
        let tab = browser.new_tab().unwrap();

        let host = format!(
            "{}://{}:{}",
            target.protocol,
            if !target.domain.is_empty() {
                target.domain
            } else {
                target.ip
            },
            target.port
        );

        if let Ok(tab) = tab.navigate_to(&host) {
            thread::sleep(time::Duration::from_secs(30));
            let jpeg_data = tab
                .capture_screenshot(ScreenshotFormat::JPEG(Some(75)), None, true)
                .unwrap();
            let mut file =
                File::create(Path::new("data/screenshots/").join(&(id + ".jpg"))).unwrap();
            file.write_all(&jpeg_data).unwrap();
        }
    });
}
