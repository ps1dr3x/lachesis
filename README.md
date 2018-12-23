# Lachesis

[Lachesis](https://en.wikipedia.org/wiki/Lachesis) is a work in progress web services mass scanner written in Rust.

This project was born as a simple test of the Rust's networking (and asynchronous I/O) performance, but later expanded with the intention to create a sort of "little personal Shodan", an open (and hopefully very fast) crawler that exposes the magnitude of outdated, vulnerable, misconfigured services publicly accessible around the web.

```
-------------8<-------------
.          .                 
|  ,-. ,-. |-. ,-. ,-. . ,-. 
|  ,-| |   | | |-' `-. | `-. 
`' `-^ `-' ' ' `-' `-' ' `-'
                      v0.1.0
-------------8<-------------

Usage:

    lachesis --dataset <dataset.json> [...optional arguments]
    lachesis --subnet <192.168.0.1/24> [--subnet <192.168.1.1/24>] [...optional arguments]

Mandatory arguments:

    --dataset <dataset.json>
        Description:
            The full path of the DNS dataset used for the requests. The accepted format is:

            {"name":"example.com","type":"a","value":"93.184.216.34"}
            {"name":"example.net","type":"a","value":"93.184.216.34"}
            {"name":"example.org","type":"a","value":"93.184.216.34"}

            An example of a compatible dataset is the forward DNS dataset by Rapid7 (https://opendata.rapid7.com/sonar.fdns_v2/)
    
    --subnet <192.168.0.1/24> [--subnet <192.168.1.1/24>]
        Description:
            Scan one or more subnets

Optional arguments:

    --def <file1> [--def <file2>] (default: all the files in resources/definitions)
        Description:
            - Multiple definitions can be selected (eg. --def wordpress --def vnc)
            - Accepted formats are:
                - File name with or without extension (eg. vnc.json or vnc). The json file will be searched in directory resources/definitions/
                - Full/relative path to file (eg. resources/definitions/vnc.json or /casual_path/mydef.json)
    --max-targets <NUM> (default: ∞)
    --debug
    --print-records
```

## Roadmap / TODOs

- Optimise https, http and tcp requests to minimize unnecessary overheads
- Add much more definitions
- Expand the scanner's capabilities to other configurations and scanning methods
- Do some information gathering on the host after each finding
- Serve a web app that shows the services stored in the database on a map

## Build from source

### Dependencies

- [Rust](https://rustup.rs/) (>1.31)

### Compile and run

```bash
cargo run -- --help
```

### Production build

```bash
./build-release.sh
```