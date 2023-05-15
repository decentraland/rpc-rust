//! Transports out of the box for the communications between two ends using Decentraland RPC.
//!
//! The Decentraland RPC implementation uses protobuf for the messages format and uses whatever transport or wire that meet the requirements of the [`Transport`] trait.
//!
use async_trait::async_trait;

pub mod error;
#[cfg(feature = "memory")]
pub mod memory;
#[cfg(feature = "websockets")]
pub mod web_sockets;

pub type TransportMessage = Vec<u8>;

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
    async fn receive(&self) -> Result<TransportMessage, TransportError>;
    async fn send(&self, message: TransportMessage) -> Result<(), TransportError>;
    async fn close(&self);
}
