#!/bin/bash

# Extract the project name and version from Cargo.toml
project_name=$(grep '^name' Cargo.toml | sed 's/name = "\(.*\)"/\1/' | tr -d '[:space:]')
version=$(grep '^version' Cargo.toml | sed 's/version = "\(.*\)"/\1/' | tr -d '[:space:]')

# Define architectures
architectures=("aarch64-apple-darwin" "x86_64-apple-darwin")

# Build for each architecture
for arch in "${architectures[@]}"; do
    cargo build --release --target=$arch
    # Extract architecture for naming
    short_arch=$(echo $arch | sed 's/-apple-darwin//')
    # Move the binary to the release directory with new naming convention
    mkdir -p ./target/release/v${version}
    mv ./target/$arch/release/$project_name ./target/release/v${version}/${project_name}.darwin-${short_arch}
done

# Change directory to release
cd ./target/release/v${version} || exit

# Create tar.gz and delete original binaries
for arch in "${architectures[@]}"; do
    short_arch=$(echo $arch | sed 's/-apple-darwin//')
    binary_name="${project_name}.darwin-${short_arch}"
    mv ${binary_name} ${project_name}
    tar -czf "${binary_name}.tar.gz" "${project_name}"
    rm "${project_name}"
done
