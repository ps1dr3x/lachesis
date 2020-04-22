#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate clap;
#[macro_use]
extern crate validator_derive;
#[macro_use]
extern crate rocket;

mod db;
mod detector;
mod lachesis;
mod stats;
mod utils;
mod validators;
mod web;
mod worker;

use colored::Colorize;
use std::process;
use unindent::unindent;

fn logo() {
    println!(
        "{}",
        unindent(
            "


        -------------8<-------------\x1b[1;36m
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-'\x1b[0;36m
                              v0.2.0\x1b[0m
        -------------8<-------------

    "
        )
    );
}

fn run_lachesis() -> Result<(), i32> {
    logo();

    // Get & check cli parameters
    let conf = match utils::get_conf() {
        Ok(conf) => conf,
        Err(err) => {
            println!("\n[{}] {}", "ERROR".red(), err);
            return Err(1);
        }
    };

    lachesis::run(&conf)
}

fn main() {
    process::exit(match run_lachesis() {
        Ok(_) => 0,
        Err(exit_code) => exit_code,
    });
}
