# Lachesis

[Lachesis](https://en.wikipedia.org/wiki/Lachesis) is a work-in-progress web services mass scanner written in Rust.

This project was born as a simple test of the Rust's asynchronous networking performance, but later expanded in an effort to create a sort of "little personal Shodan", an open tool that exposes the magnitude of outdated, vulnerable, misconfigured services publicly accessible around the web.

## Roadmap / TODOs

- Optimise https, http and tcp requests handling
- Expand the scanner's capabilities to other configurations and scanning methods
- Do some information gathering on the host after each finding
- Serve a web app that shows the services stored in the database on a map
