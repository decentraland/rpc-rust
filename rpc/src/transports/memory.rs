//! MemoryTransport has no common use case in a server or web app but it's great for testing propouses.
//!
//! However, this type of transport has an use case on Decentraland for the comms between `Scenes<>BrowserInterface<>GameEngine`.
//!
//! The most common use case is for testing. It uses [`async_channel`] internally
use super::{Transport, TransportError, TransportMessage};
use async_channel::{bounded, Receiver, Sender};
use async_trait::async_trait;

/// Using channels for the communication between a `RpcClient` and `RpcServer` running on the same process.
pub struct MemoryTransport {
    /// The sender half of an [`async_channel::bounded`] channel
    ///
    /// It uses an [`async_channel`] to meet the requirements of the [`Transport`] trait
    ///
    /// eg: [`RpcClient`](crate::client::RpcClient) stores the sender half of the channel which the [`RpcServer`](crate::server::RpcServer)  stores the receiver half and viceversa
    sender: Sender<Vec<u8>>,
    /// The receiver half of an [`async_channel::bounded`] channel
    ///
    /// It uses an [`async_channel`] to meet the requirements of the [`Transport`] trait
    ///
    /// eg: [`RpcClient`](crate::client::RpcClient) stores the receiver half of the channel which the [`RpcServer`](crate::server::RpcServer)  stores the sender half and viceversa
    receiver: Receiver<Vec<u8>>,
}

impl MemoryTransport {
    fn new(sender: Sender<Vec<u8>>, receiver: Receiver<Vec<u8>>) -> Self {
        Self { sender, receiver }
    }

    /// It creates two [`MemoryTransport`]s for the both ends using [`async_channel::bounded`]
    ///
    /// The first element in the tuple is the transport for the [`RpcClient`](crate::client::RpcClient) and the second one for the [`RpcServer`](crate::server::RpcServer)
    pub fn create() -> (Self, Self) {
        let (client_sender, server_receiver) = bounded::<Vec<u8>>(32);
        let (server_sender, client_receiver) = bounded::<Vec<u8>>(32);

        let client = Self::new(client_sender, client_receiver);
        let server = Self::new(server_sender, server_receiver);

        (client, server)
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn receive(&self) -> Result<TransportMessage, TransportError> {
        match self.receiver.recv().await {
            Ok(message) => Ok(message),
            Err(_) => {
                self.close().await;
                Err(TransportError::Closed)
            }
        }
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        match self.sender.send(message).await {
            Ok(_) => Ok(()),
            Err(_) => Err(TransportError::Closed),
        }
    }

    async fn close(&self) {
        self.receiver.close();
    }
}
