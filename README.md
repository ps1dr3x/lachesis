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


Lachesis v0.1.0
Michele Federici (@ps1dr3x) <michele@federici.tech>

USAGE:
    lachesis [FLAGS] [OPTIONS] --dataset <FILE> --subnet <SUBNET>... --web-ui

FLAGS:
    -v, --debug      Print debug messages
    -h, --help       Prints help information
    -V, --version    Prints version information
    -w, --web-ui     Serve a web app (and a basic API) to visualize/explore collected data
                      

OPTIONS:
    -D, --dataset <FILE>           The full path of the DNS dataset used for the requests. The accepted format is:
                                   
                                   {"name":"example.com","type":"a","value":"93.184.216.34"}
                                   {"name":"example.net","type":"a","value":"93.184.216.34"}
                                   {"name":"example.org","type":"a","value":"93.184.216.34"}
                                   
                                   An example of a compatible dataset is the forward DNS dataset by Rapid7
                                   (https://opendata.rapid7.com/sonar.fdns_v2/)
                                    
    -d, --def <FILE>...            Default: all the files in resources/definitions
                                    
                                   Multiple definitions can be selected (eg. --def wordpress --def vnc)
                                   Accepted formats are:
                                     File name with or without extension (eg. vnc.json or vnc). The json file will be
                                   searched in directory resources/definitions/
                                     Full/relative path to file (eg. resources/definitions/vnc.json or
                                   /casual_path/mydef.json)
                                      
    -e, --exclude-def <FILE>...    If all the existing definitions are selected (no -d/--def values provided) is
                                   possible to exclude some of them using this argument.
                                   Accepted formats are:
                                     File name with or without extension (eg. vnc.json or vnc)
                                      
    -m, --max-targets <NUM>        Sets a maximum limit of targets
                                    
    -S, --subnet <SUBNET>...       Scan one or more subnets
```

## Roadmap / TODOs

- Optimise https, http and tcp requests to minimize unnecessary overheads
- Add much more definitions
- Expand the scanner's capabilities to other configurations and scanning methods
- Do some information gathering on the host after each finding
- Improve the API and the Web UI (a geo map would be nice)

## Build from source

### Dependencies

- [Rust](https://rustup.rs/): Usually nightly is needed
- [Node.js, Npm](https://nodejs.org): Needed for the Web UI (front end) part
- On Linux and BSD based OS: OpenSSL (libssl-dev on deb, openssl-devel on rpm)

### Compile and run (development)

#### Web UI

If you don't intend to work on the Web UI (front end) part, you can do this only once. If you don't intend to use the Web UI, this can be skipped.

```bash
cd src/ui
npm install
npm run build # or npm run watch
```

#### Lachesis

```bash
cargo run -- --help
```

### Production build (Web UI + Lachesis)

```bash
./scripts/build-release.sh
```

### Troubleshooting

#### "Too many open files" error

Some Linux distributions are configured with a very low limit to the number of maximum opened files. In order to run this software properly, it could be needed to increment it.

The following limits are only an example. Depending on the machine configuration and overall load they can be set higher or lower, only for an user or only for root

- PAM (/etc/security/limits.conf)
  ```
  * - nofile 99999 # or username - nofile 99999
  root soft nofile 99999
  root hard nofile 99999
  ```
- Systemd (/etc/systemd/user.conf and /etc/systemd/system.conf)
  ```
  DefaultLimitNOFILE=99999
  ```

Note: To make the modification effective, a reboot is needed. The current limits can be checked using the commands:

```bash
ulimit -Hn #hard
ulimit -Sn #soft
```
