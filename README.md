# Lachesis

[Lachesis](https://en.wikipedia.org/wiki/Lachesis) is a web services mass scanner written in Rust.

This project started as a test of Rust's async networking performance and grew into a "little personal Shodan", an open scanner that collects statistical data on web services and helps surface outdated, vulnerable, or misconfigured services publicly accessible on the internet.

## Features

- Async, high-concurrency scanning
- TCP port probing with dynamic timeout estimation (nmap-style RTT)
- HTTP/HTTPS and raw TCP/custom protocol requests
- Extensible JSON definition files for service detection and version fingerprinting
- PostgreSQL persistence with deduplication and seen-count tracking
- Web UI and REST API to explore findings
- Subnet scanning (`--subnet`) or DNS dataset scanning (`--dataset`)

## Definitions

Detection rules live in `resources/definitions/`. Each file is a JSON array of definitions with regex-based service and version matching.

## Usage

```
-------------8<-------------
.          .                 
|  ,-. ,-. |-. ,-. ,-. . ,-. 
|  ,-| |   | | |-' `-. | `-. 
`' `-^ `-' ' ' `-' `-' ' `-'
                      v0.4.0
-------------8<-------------

Web services mass scanner

Usage: lachesis [OPTIONS]

Options:
  -D, --dataset <FILE>                 The full path of the DNS dataset used for the requests. JSONL, one record per line. An example of a compatible dataset is the forward DNS dataset by Rapid7 (https://opendata.rapid7.com/sonar.fdns_v2/). Example format of each line: {"name":"example.com","type":"a","value":"1.2.3.4"}
  -S, --subnet <SUBNET>...             Scan one or more subnets (e.g. --subnet 10.0.0.0/24 --subnet 192.168.1.0/24)
  -d, --def <FILE>...                  Definition file(s) to use. Default: all files in resources/definitions/
  -e, --exclude-def <FILE>...          Exclude specific definition file(s) (only when no --def is given)
  -u, --user-agent <STRING>            Custom user agent string for HTTP/HTTPS requests [default: lachesis/0.4.0]
  -m, --max-targets <NUM>              Maximum number of targets to scan
  -t, --req-timeout <NUM>              Maximum timeout per request in seconds [default: 10]
  -c, --max-concurrent-requests <NUM>  Maximum number of concurrent requests (0 = unlimited) [default: 0]
  -v, --debug                          Print debug messages
  -w, --web-ui                         Serve a web app (and a basic API) to visualize/explore collected data
  -r, --max-response-size <BYTES>      Maximum response body size in bytes (HTTP and TCP) [default: 10240]
  -h, --help                           Print help
  -V, --version                        Print version
```

## Build from source

### Dependencies

- [Rust](https://rustup.rs/) — stable toolchain
- [Node.js / npm](https://nodejs.org) — for the Web UI frontend
- On Linux/BSD:
  - `pkg-config`, `libssl-dev` (Debian/Ubuntu) or `openssl-devel` (RHEL/Fedora)
- [Docker + Docker Compose](https://www.docker.com/) — for running the test database

### Build

```bash
# Frontend (only needed once, or when UI changes)
npm install
npm run build   # or: npm run watch

# Backend
cargo build --release
```

### Development

```bash
cargo run -- --help
cargo run -- --subnet 192.168.1.0/24 --def redis --debug
cargo run -- --web-ui
```

### Production release

```bash
./scripts/build-release.sh
```

### Tests

```bash
docker-compose up -d
cargo test
```

## Troubleshooting

### "Too many open files"

Some Linux distributions set a low default limit on open file descriptors. Increase it in:

- `/etc/security/limits.conf`:
  ```
  * - nofile 99999
  root soft nofile 99999
  root hard nofile 99999
  ```
- `/etc/systemd/user.conf` and `/etc/systemd/system.conf`:
  ```
  DefaultLimitNOFILE=99999
  ```

Check current limits with `ulimit -Hn` (hard) and `ulimit -Sn` (soft). A reboot is recommended after changing these.

## Roadmap

- Plugin/API system to extend scanner capabilities
- Geographic information for findings (GeoIP lookup)
- Web UI map view showing findings geographically
- Result filtering in the Web UI
- Additional HTTP methods and request payloads in definitions
- `exclude_if` option in definitions (min page size, regex blacklist)
- Dataset read modes: forward, backward, random (configurable)
- Read subnets from file
- Continuous scanning / agent mode
