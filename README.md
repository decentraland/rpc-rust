# RPC Rust

[![Build]https://github.com/decentraland/rpc-rust/workflows/Validations/badge.svg)](
https://github.com/decentraland/rpc-rust/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue.svg)](
https://github.com/decentraland/rpc-rust)
[![Cargo](https://img.shields.io/crates/v/dcl-rpc.svg)](
https://crates.io/crates/dcl-rpc)
[![Documentation](https://docs.rs/dcl-rpc/badge.svg)](
https://docs.rs/dcl-rpc)

## Requirements

Install protoc binaries

### MacOS
```bash
brew install protobuf
```

### Debian-based Linux
```bash
sudo apt-get install protobuf-compiler
```

### Install Just for commands
```bash
cargo install just
```

## Build

`cargo build`

## Run the integration example

`just run-integration`

## Run the multi language integration example

`just run-multilang`