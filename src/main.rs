#[macro_use]
extern crate clap;
#[macro_use]
extern crate validator_derive;

mod lachesis;
mod worker;
mod detector;
mod utils;
mod db;
mod stats;
mod validators;

use std::process;
use unindent::unindent;
use colored::Colorize;
use crate::lachesis::lachesis;

fn logo() {
    println!("{}", unindent("


        -------------8<-------------\x1b[1;36m
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-'\x1b[0;36m
                              v0.1.0\x1b[0m
        -------------8<-------------

    "));
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

    // If --print-records/-p option is specified. Print records and exit
    if conf.print_records {
        match utils::print_records() {
            Ok(()) => return Ok(()),
            Err(err) => {
                println!("\n[{}] Embedded db error: {}\n", "ERROR".red(), err);
                return Err(1);
            }
        }
    }

    lachesis(&conf)
}

fn main() {
    process::exit(match run_lachesis() {
        Ok(_) => 0,
        Err(exit_code) => exit_code
    });
}
