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

## Examples

### Run the integration example

RPC Client in Rust and RPC Server in Rust running Websocket transport example, Memory Transport example and example using different types of transports

`just run-integration`

### Run the integration example with an specific transport 

RPC Client in Rust and RPC Server in Rust running the example passed to the command

`just run-integration {ws|memory|dyn}`

### Run the multi language integration example 

RPC Client in Typescript and RPC Server in Rust using WebSockets

`just run-multilang`

You can find the code for these examples in the `examples/` directory.

## Usage

### Import

```toml
[dependencies]
dcl-rpc = "*"

[build-dependencies]
prost-build = "*"
dcl-rpc = "*" # As a build depency as well because we need the codegen module for the code-generation of the defined RPC Service in the .proto
```

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
use std::io::Result;

fn main() -> Result<()> {
    // Tell Cargo that if the given file changes, to rerun this build script.
    println!("cargo:rerun-if-changed=src/echo.proto");

    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(dcl_rpc::codegen::RPCServiceGenerator::new()));
    conf.compile_protos(&["src/echo.proto"], &["src"])?;
    Ok(())
}
```

The `build.rs` script runs every time that your `.proto` changes. The script will generate a file in the `OUT_DIR`, named as the `package` field in the `.proto` file (if it's not declared, the name will be '_.rs'). This file will include: 
- All your declared messages in the `.proto` as Rust structs. *1
- (`#[cfg(feature = "server")]`) A trait, named `{YOUR_RPC_SERVICE_NAME}Server: Send + Sync + 'static`, with the methods defined in your service for the server side. 
So you should use this trait to build an implementation with the business logic. *2
- (`#[cfg(feature = "client")]`) A trait, named `{YOUR_RPC_SERVICE_NAME}ClientDefinition<T: Transport + 'static>: ServiceClient<T> + Send + Sync + 'static`, and an implementation of it for the client side, named `{YOUR_RPC_SERVICE_NAME}Client`. 
You could use this auto-generated implementation when using the `RpcClient` passing the implementation (struct with the trait implemented) as a generic in the `load_module` function, which it'll be in charge of requesting the procedures of your service. 
But you could also have your own implementation of the `{YOUR_RPC_SERVICE_NAME}ClientDefinition` trait, as long as the implementations meets with trait's and `RpcClient` requirements .  *3
- (`#[cfg(feature = "server")]`)  A struct in charge of registering your declared service when a `RpcServerPort` is created. 
You should use this struct and its registering function inside the `RpcServer` port creation handler. *4

To import them you must add:

```rust
include!(concat!(env!("OUT_DIR"), "/decentraland.echo.rs"));
```

This statement should be added to the `src/lib.rs` in order to make the auto-generated code part of your crate, otherwise it will treat every include as different types.

### Server Side

```rust
use dcl_rpc::{
    transports::web_socket::{WebSocketServer, WebSocketTransport}, 
    server::{RpcServer, RpcServerPort}, 
    service_module_definition::{Definition, ServiceModuleDefinition, CommonPayload}
};

use crate::{
    EchoServiceRegistration, // (*4)
};

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
  // The EchoServiceRegistration will be autogenerated, so you'll need to define the echo_service, which will have all the behaviors of your service. Following the example, it'll have the logic for the `hello` message.
  EchoServiceRegistration::register_service(port, echo_service::MyEchoService {})
});

// The WebSocket Server listens for incoming connections, when a connection is established, it creates a new WebSocketTransport with that connection and attaches it to the server event sender. The loop continues to listen for incoming connections and attach transports until it is stopped.
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
use crate::{
    MyExampleContext, 
    EchoServiceServer, // (*2)
    Text // (*1) message
};

pub struct MyEchoService;

#[async_trait::async_trait]
impl EchoServiceServer<MyExampleContext> for MyEchoService {
    async fn hello(&self, request: Text, _ctx: Arc<MyExampleContext>) -> Text {
        request
    }
}
```

### Client Side

Initiate a WebSocket Client Connection and send a Hello World message to the echo server.

```rust
use crate::{EchoServiceClient, RPCServiceClient} // (*3)
use dcl_rpc::{transports::web_socket::{WebSocketClient, WebSocketTransport}, client::RpcClient};
use ws_rust::Text;

let client_connection = WebSocketClient::connect("ws://localhost:8080")
    .await
    .unwrap();

let client_transport = WebSocketTransport::new(client_connection);
let mut client = RpcClient::new(client_transport).await.unwrap();
let port = client.create_port("echo").await.unwrap();

let module = port.load_module::<EchoServiceClient>("EchoService").await.unwrap();
let response = module.hello(Text { say_something: "Hello World!".to_string()}).await;
```
