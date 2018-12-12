mod lachesis;
mod worker;
mod detector;
mod utils;
mod db;
mod stats;

use std::process;
use unindent::unindent;
use crate::lachesis::lachesis;

fn logo() {
    println!("{}", unindent("

        
        .          .                 
        |  ,-. ,-. |-. ,-. ,-. . ,-. 
        |  ,-| |   | | |-' `-. | `-. 
        `' `-^ `-' ' ' `-' `-' ' `-' 
                              v0.1.0

    "));
}

fn usage() {
    println!("{}", unindent("

        Usage:

            lachesis --dataset <dataset.json> [...optional arguments]
            lachesis --ip-range <192.168.1.1 192.168.2.100> [...optional arguments]

        Mandatory arguments:

            --dataset <dataset.json>
                Description:
                    The full path of the DNS dataset used for the requests. The accepted format is:

                    {\"name\":\"example.com\",\"type\":\"a\",\"value\":\"93.184.216.34\"}
                    {\"name\":\"example.net\",\"type\":\"a\",\"value\":\"93.184.216.34\"}
                    {\"name\":\"example.org\",\"type\":\"a\",\"value\":\"93.184.216.34\"}

                    An example of a compatible dataset is the forward DNS dataset by Rapid7 (https://opendata.rapid7.com/sonar.fdns_v2/)
            
            --ip-range <192.168.1.1 192.168.2.100>
                Description:
                    An ip range

        Optional arguments:

            --def <file> (default: all the files in resources/definitions)
                Description:
                    - Multiple definitions can be selected (eg. --def wordpress --def vnc)
                    - Accepted formats are:
                        - File name with or without extension (eg. vnc.json or vnc). The json file will be searched in directory resources/definitions/
                        - Full/relative path to file (eg. resources/definitions/vnc.json or /casual_path/mydef.json)
            --threads <num> (default: 4)
            --max-targets <NUM> (default: 1000)
            --debug
            --print-records

    "));
}

fn run_lachesis() -> Result<(), i32> {
    // Print lachesis logo
    logo();

    // Get & check cli parameters
    let conf = match utils::get_cli_params() {
        Ok(conf) => conf,
        Err(err) => {
            println!("\n[ERROR] {}", err);
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
        match utils::print_records() {
            Ok(()) => return Ok(()),
            Err(err) => {
                println!("\n[ERROR] Embedded db error: {}\n", err);
                return Err(1);
            }
        }
    }

    // Let's go!
    lachesis(&conf)
}

fn main() {
    process::exit(match run_lachesis() {
        Ok(_) => 0,
        Err(exit_code) => exit_code
    });
}
