//! Transports out of the box for the communications between two ends using Decentraland RPC.
//!
//! The Decentraland RPC implementation uses protobuf for the messages format and uses whatever transport or wire that meet the requirements of the [`Transport`] trait.
//!
use async_trait::async_trait;

pub mod error;
pub mod memory;
pub mod quic;
pub mod web_socket;

#[derive(Debug)]
pub enum TransportEvent {
    ///
    /// The connect event is emited when the transport gets connected.
    ///
    /// The RpcServer is in charge to send the notification
    /// to signal the client transport that it is connected.
    ///
    Connect,
    /// the on_message callback is called when the transport receives a message
    Message(Vec<u8>),
    /// the error event is emited when the transport triggers an error
    Error(String),
    /// the close function will be called when it is decided to end the communication
    Close,
}

#[derive(Debug)]
pub enum TransportError {
    /// Error while the underlying transport is running.
    ///
    /// For example: A peer reset the connection in a websocket connection
    ///
    Internal(Box<dyn std::error::Error + Send + Sync>),
    /// Transport is already closed
    Closed,
    /// When the received message is not a binary
    NotBinaryMessage,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn receive(&self) -> Result<TransportEvent, TransportError>;
    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError>;
    async fn close(&self);
    fn is_connected(&self) -> bool;
}
