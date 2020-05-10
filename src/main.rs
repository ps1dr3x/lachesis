#![feature(try_trait, termination_trait_lib, proc_macro_hygiene, decl_macro)]

#[macro_use]
extern crate clap;
#[macro_use]
extern crate validator_derive;
#[macro_use]
extern crate rocket;

mod browser;
mod conf;
mod db;
mod detector;
mod lachesis;
mod net;
mod stats;
mod utils;
mod validators;
mod web;
mod worker;

use unindent::unindent;

use crate::lachesis::ExitCode;

fn main() -> ExitCode {
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

    lachesis::run()
}
