pub mod client;
pub mod messages_handlers;
pub mod rpc_protocol;
pub mod server;
pub mod service_module_definition;
pub mod stream_protocol;
pub mod transports;

#[derive(Debug)]
pub enum CommonError {
    ProtocolError,
    TransportError,
    TransportNotAttached,
}
