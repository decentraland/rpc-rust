
use async_trait::async_trait;

pub mod memory;

#[derive(Debug)]
pub enum TransportEvent {
    ///
    /// The connect event is emited when the transport gets connected.
    ///
    /// The RpcServer is in charge to send the notification (bytes[1]\{0x0\})
    /// to signal the client transport that it is connected.
    ///
    Connect,
    /// the on_message callback is called when the transport receives a message
    Message { data: Vec<u8> },
    /// the error event is emited when the transport triggers an error
    Error(String),
    /// the close function will be called when it is decided to end the communication
    Close,
}

#[derive(Debug)]
pub enum TransportError {
    Connection,
    Internal,
}

#[async_trait]
pub trait Transport {
    async fn receive(&mut self) -> Result<TransportEvent, TransportError>;
    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError>;
    fn close(&mut self);
}

