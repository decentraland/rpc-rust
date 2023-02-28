# dcl-rpc

[![Build](https://github.com/decentraland/rpc-rust/workflows/Validations/badge.svg)](
https://github.com/decentraland/rpc-rust/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue.svg)](
https://github.com/decentraland/rpc-rust)
[![Cargo](https://img.shields.io/crates/v/dcl-rpc.svg)](
https://crates.io/crates/dcl-rpc)
[![Documentation](https://docs.rs/dcl-rpc/badge.svg)](
https://docs.rs/dcl-rpc)

The Rust implementation of Decentraland RPC. At Decentraland, we have our own implementation of RPC for communications between the different services.

Currently, there are other implementations: 
- [Typescript](https://github.com/decentraland/rpc) 
- [C#](https://github.com/decentraland/rpc-csharp).

## Requirements

- Install protoc binaries

### MacOS
```bash
brew install protobuf
```

### Debian-based Linux
```bash
sudo apt-get install protobuf-compiler
```

- Install Just

### Install Just for commands
```bash
cargo install just
```

## Build

`cargo build`

## Examples

### Run the integration example: RPC Client in Rust and RPC Server in Rust

`just run-integration`

### Run the multi language integration example: RPC Client in Typescript and RPC Server in Rust

`just run-multilang`