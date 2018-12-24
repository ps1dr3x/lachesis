#!/bin/bash

# Exit if a command fails
set -e

# Get current version from Cargo.toml
VERSION=$(grep -oP '(?<=version = ")(\d\.\d\.\d)(?=")' Cargo.toml)
echo "Building Lachesis $VERSION"

# Create a directory for the version
mkdir -p dist/lachesis-$VERSION

# Compile
cargo build --release
cp target/release/lachesis dist/lachesis-$VERSION

# Copy bin file and resources into the dist dir
cp -R resources dist/lachesis-$VERSION

# Create the db directory
mkdir dist/lachesis-$VERSION/db

# Notify success
echo "Build succeeded: dist/lachesis-$VERSION"
