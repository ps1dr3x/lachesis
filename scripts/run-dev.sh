#!/bin/bash

time env RUST_BACKTRACE=1 cargo run -- $(for word in "$*"; do echo "$word"; done) |& tee logs/$(date +%s)-run.log