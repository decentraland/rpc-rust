//! Rust Implementation of Decentraland RPC.
//!
//! Communicates between different services written in different languages, just sharing a `.proto` file.
//! Decentraland RPC implementation uses protobuffer as the messaging format and you can create custom transports (muest meet [Transport trait][`crate::transports::Transport`] requirements) or use the existing ones for communication.
//!
//! The communication is carried out by a client (or many ones) and a server. A [`RpcClient`](crate::client::RpcClient) connects to a [`RpcServer`](crate::server::RpcServer) . A `RpcServer` accepts multiple clients.
//! For example, the `RpcServer` could have a WebSocket server listening for new connections for then [attaching][`crate::server::RpcServer::attach_transport`] each new connection socket to the `RpcServer`.
//! And on the client side, a connection to the WebSocket server could be established for then creating a new `RpcClient` passing the connection socket generated.
//!
//! After that, the `RpcClient` should create a [`RpcClientPort`](crate::client::RpcClientPort) by calling [`RpcClient::create_port`][`crate::client::RpcClient::create_port`], this sends a request to the `RpcServer` to create a [`crate::server::RpcServerPort`] in the server too.
//! A created port is "detached" from the client and the server. A port can be closed or destroyed but the client (`RpcClient`) will keep existing, also the server (`RpcServer`), just the port will be removed from the client's memory and the server's memory.
//!
//! On the server side, each port created owns different types of modules (A `service` in `.proto` file). Each port has its registered modules and loaded modules.
//! Every time that a port is created, on the server side, a handler is executed to register a module or multiple ones. Also, the handler could have conditions to register a module depending on specific conditions like a port name or something else.
//!
//! On the client side, each port created can request remote modules (A 'service' in `.proto` file) to the `RpcServer` as long as the port has the given module registered or loaded. A client could create and have multiple ports, each procedure request has the id of the port that is making the request.
//!
//! The `RpcServer` shares a context through all its registered modules. This context should have the app context, like a database connection/component. Every procedure can access and use the server context, and the same instance of the context is shared between them (`Arc<Context>`).
//!
//! You could find basic and complex examples in the [examples folder](https://github.com/decentraland/rpc-rust/tree/main/examples) on the [github repository](https://github.com/decentraland/rpc-rust).
//!

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "codegen")]
pub mod codegen;
pub mod messages_handlers;
pub mod rpc_protocol;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod service_module_definition;
pub mod stream_protocol;
pub mod transports;

#[derive(Debug)]
pub enum CommonError {
    ProtocolError,
    TransportError,
    TransportNotAttached,
    UnexpectedError(String),
    TransportWasClosed,
}
