extern crate easy_reader;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate unindent;

mod lachesis;
mod worker;
mod detector;
mod utils;
mod db;
mod stats;

use std::process;
use unindent::unindent;
use lachesis::lachesis;

fn usage() {
    println!("{}", unindent("

        Usage:

        lachesis --file dns.json [...optional arguments]

        Optional arguments:

        --threads NUM (default: 4)
        --max-targets NUM (default: 1000)
        --debug
        --print-records

    "));
}

fn run_lachesis() -> Result<(), i32> {
    println!("{}", unindent("

        
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-' 
                              v0.1.0

    "));

    // Get & check cli parameters
    let conf = match utils::get_cli_params() {
        Ok(conf) => conf,
        Err(err) => {
            println!("{}", err);
            usage();
            return Err(1);
        }
    };

    // --help option specified. Print usage and exit
    if conf.help {
        usage();
        return Ok(());
    }

    // --print-records option specified. Print records and exit
    if conf.print_records {
        utils::print_records();
        return Ok(());
    }

    // Let's go!
    lachesis(conf)
}

fn main() {
    process::exit(match run_lachesis() {
       Ok(_) => 0,
       Err(exit_code) => exit_code
    });
}
