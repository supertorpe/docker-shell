#!/bin/bash

# Ensure we exit immediately if any command fails
set -e

echo "Building Rust binary inside Docker..."

mkdir -p .cache

# Run the official Rust container, mounting our current directory
docker run --rm \
    -u $(id -u):$(id -g) \
    -v "$PWD":/volume \
    -v .cache:/usr/local/cargo/registry \
    -w /volume \
    rust:1.88-slim \
    cargo build --release

echo "Build complete! Your binary is available at:"
echo "./target/release/docker-shell"
