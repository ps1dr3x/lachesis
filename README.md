# Lachesis

[Lachesis](https://en.wikipedia.org/wiki/Lachesis) is a work in progress web services mass scanner written in Rust.

This project was born as a simple test of the Rust's networking (and asynchronous I/O) performance, but later expanded with the intention to create a sort of "little personal Shodan", an open scanner that collects statistical data on web services and exposes the magnitude of outdated, vulnerable, misconfigured services publicly accessible around the web.

```
-------------8<-------------
.          .                 
|  ,-. ,-. |-. ,-. ,-. . ,-. 
|  ,-| |   | | |-' `-. | `-. 
`' `-^ `-' ' ' `-' `-' ' `-'
                      v0.3.0
-------------8<-------------


Lachesis v0.3.0
Michele Federici (@ps1dr3x) <michele@federici.tech>

USAGE:
    lachesis [FLAGS] [OPTIONS] --dataset <FILE> --subnet <SUBNET>... --web-ui

FLAGS:
    -v, --debug      Print debug messages
    -h, --help       Prints help information
    -V, --version    Prints version information
    -w, --web-ui     Serve a web app (and a basic API) to visualize/explore collected data
                      

OPTIONS:
    -D, --dataset <FILE>                   The full path of the DNS dataset used for the requests. The accepted format
                                           is:
                                           
                                           {"name":"example.com","type":"a","value":"93.184.216.34"}
                                           {"name":"example.net","type":"a","value":"93.184.216.34"}
                                           {"name":"example.org","type":"a","value":"93.184.216.34"}
                                           
                                           An example of a compatible dataset is the forward DNS dataset by Rapid7
                                           (https://opendata.rapid7.com/sonar.fdns_v2/)
                                            
    -d, --def <FILE>...                    Default: all the files in resources/definitions
                                            
                                           Multiple definitions can be selected (eg. --def wordpress --def vnc)
                                           Accepted formats are:
                                             File name with or without extension (eg. vnc.json or vnc). The json file
                                           will be searched in directory resources/definitions/
                                             Full/relative path to file (eg. resources/definitions/vnc.json or
                                           /casual_path/mydef.json)
                                              
    -e, --exclude-def <FILE>...            If all the existing definitions are selected (no -d/--def values provided) is
                                           possible to exclude some of them using this argument.
                                           Accepted formats are:
                                             File name with or without extension (eg. vnc.json or vnc)
                                              
    -c, --max-concurrent-requests <NUM>    Sets a maximum number of concurrent requests
                                            [default: 0]
    -m, --max-targets <NUM>                Sets a maximum limit of targets
                                            
    -t, --req-timeout <NUM>                Sets a maximum timeout for each request (seconds)
                                            [default: 10]
    -S, --subnet <SUBNET>...               Scan one or more subnets
                                            
    -u, --user-agent <STRING>              Sets a custom user agent (http/https)
                                            [default: lachesis/0.3.0]
```

## Roadmap / TODOs

- Optimise https, http and tcp requests to minimize unnecessary overheads
- Add much more definitions
- Plugin system to expand the scanner's capabilities to other configurations, request types and scanning methods
- Do some information gathering on the host after each finding
- Improve the API and the Web UI (a geo map would be nice)
- Distributed DB/agent mode

## Build from source

### Dependencies

- [Rust](https://rustup.rs/): Usually nightly is needed
- [Node.js, Npm](https://nodejs.org): Needed for the Web UI (front end) part
- On Linux and BSD based OS:
  - pkg-config (pkg-config on deb, pkg-config/pkgconfig/pkgconf-pkg-config on rpm)
  - libssl (libssl-dev on deb, openssl-devel on rpm)

### Compile and run (development)

#### Web UI

If you don't intend to work on the Web UI (front end) part, you can do this only once. If you don't intend to use the Web UI, this can be skipped.

```bash
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

Some Linux distributions are configured with a very low limit on the number of maximum opened files. Depending on the number of concurrent requests and other factors, that limit might be reached, crashing the software.

The limits can usually be increased in the following files. This is only an example, depending on the machine configuration and overall load they can be set higher or lower, only for an user or only for root.

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
