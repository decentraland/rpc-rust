# dcl-rpc

[![Build](https://github.com/decentraland/rpc-rust/workflows/Validations/badge.svg)](
<https://github.com/decentraland/rpc-rust/actions>)
[![License](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue.svg)](
<https://github.com/decentraland/rpc-rust>)
[![Cargo](https://img.shields.io/crates/v/dcl-rpc.svg)](
<https://crates.io/crates/dcl-rpc>)
[![Documentation](https://docs.rs/dcl-rpc/badge.svg)](
<https://docs.rs/dcl-rpc>)

The Rust implementation of Decentraland RPC. At Decentraland, we have our own implementation of RPC for communications between the different services.

Currently, there are other implementations:

- [Typescript](https://github.com/decentraland/rpc)
- [C#](https://github.com/decentraland/rpc-csharp)

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

## Usage

### Import

```toml
[dependencies]
dcl-rpc = "latest"

[build-dependencies]
prost-build = "latest"
dcl-rpc-codegen = "latest"
````

### Protobuf

Create a file `app.proto` to define the messages that will be used, for example:

```proto
syntax = "proto3";
package decentraland.echo;

message Text {
  string say_something = 1;
}

service EchoService {
  rpc Hello(Text) returns (Text) {}
}
```

Then, define a `build.rs` file to build the types of the message:

```rust
extern crate prost_build;
use std::io::Result;

fn main() -> Result<()> {
    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=src/echo.proto");

    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(dcl_rpc_codegen::RPCServiceGenerator::new()));
    conf.compile_protos(&["src/echo.proto"], &["src"])?;
    Ok(())
}
```

That code will build the necessary modules to support the custom messages defined in the protobuf and use them in Rust, and also generate the necessary code for the Service Interface, leaving to the user the implementation of the logic.

To import them you will need to add:

```rust
include!(concat!(env!("OUT_DIR"), "/decentraland.echo.rs"));
```

This include should be done in the `src/lib.rs` so the entire crate handle types correctly, otherwise it will treat every include as different types.


### Server Side

```rust
use dcl_rpc::{transports::web_socket::{WebSocketServer, WebSocketTransport}, server::{RpcServer, RpcServerPort}, service_module_definition::{Definition, ServiceModuleDefinition, CommonPayload}};

// Define the IP and Port where the WebSocket Server will run
let ws_server = WebSocketServer::new("localhost:8080");
// Start listening on that IP:PORT
let mut connection_listener = ws_server.listen().await.unwrap();

// Add here any data that the server needs to solve the messages, for example db.
let ctx = MyExampleContext {
    hardcoded_database: create_db(),
};

let mut server = RpcServer::create(ctx);
server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
  // The EchoServiceCodeGen will be autogenerated, you need to define the echo_service
  EchoServiceRegistration::register_service(port, echo_service::EchoService {})
});

// It has to use the server events sender to attach transport because it has to wait for client connections
// and keep waiting for new ones
let server_events_sender = server.get_server_events_sender();
tokio::spawn(async move {
    while let Some(Ok(connection)) = connection_listener.recv().await {
        let transport = WebSocketTransport::new(connection);
        match server_events_sender.send_attach_transport(transport) {
            Ok(_) => {
                println!("> RpcServer > transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }
    }
});

server.run().await;
```

Implement the trait for your service

```rust
use crate::{MyExampleContext, SharedEchoService, Text};

#[async_trait::async_trait]
impl SharedEchoService<MyExampleContext> for MyEchoService {
    async fn hello(&self, request: Text, _ctx: Arc<MyExampleContext>) -> Text {
        request
    }
}
```

### Client Side

Initiate a WebSocket Client Connection and send a Hello World message to the echo server.


```rust
use codegen::client::{EchoServiceClient, EchoServiceClientInterface};
use dcl_rpc::{transports::web_socket::{WebSocketClient, WebSocketTransport}, client::RpcClient};
use ws_rust::Text;
mod codegen;

let client_connection = WebSocketClient::connect("ws://localhost:8080")
    .await
    .unwrap();

let client_transport = WebSocketTransport::new(client_connection);
let mut client = RpcClient::new(client_transport).await.unwrap();
let port = client.create_port("echo").await.unwrap();

let module = port.load_module::<EchoServiceClient>("EchoService").await.unwrap();
let response = module.hello(Text { say_something: "Hello World!".to_string()}).await;
```
