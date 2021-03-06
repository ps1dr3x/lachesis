#!/bin/bash

# Exit if a command fails
set -e

# Get current version from Cargo.toml
VERSION=$(grep -oP '(?<=version = ")(\d\.\d\.\d)(?=")' Cargo.toml)
printf "\nBuilding Lachesis $VERSION\n\n"

# Create a directory for the version
mkdir dist/lachesis-$VERSION

# Change dir to the web-ui root
cd src/ui

# Install Web UI dependencies
npm install

# Run the Web UI build script
npm run build

# Back to project root
cd ../..

# Compile Lachesis
cargo build --release

# Copy bin file and resources into the dist dir
cp target/release/lachesis dist/lachesis-$VERSION
cp -R resources dist/lachesis-$VERSION

# Copy the conf directory
cp -r data/ dist/lachesis-$VERSION/conf

printf "\nBuild succeeded: dist/lachesis-$VERSION\n\n"