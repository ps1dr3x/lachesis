#[macro_use]
extern crate clap;
#[macro_use]
extern crate validator_derive;
#[macro_use]
extern crate rocket;

mod conf;
mod db;
mod detector;
mod lachesis;
mod net;
mod stats;
#[cfg(test)]
mod test;
mod validators;
mod web;
mod worker;

use unindent::unindent;

fn main() {
    println!(
        "{}",
        unindent(
            "


        -------------8<-------------\x1b[1;36m
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-'\x1b[0;36m
                              v0.3.0\x1b[0m
        -------------8<-------------

    "
        )
    );

    std::process::exit(match lachesis::run() {
        Ok(_) => 0,
        Err(_) => 1,
    });
}
