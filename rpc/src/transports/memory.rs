use std::sync::atomic::{AtomicBool, Ordering};

use async_channel::{bounded, Receiver, Sender};
use async_trait::async_trait;

use super::{Transport, TransportError, TransportEvent};

/// MemoryTransport usually has no a use case in a real project (maybe if it's compiled to WASM and used in the browser).
///
/// The most common use case is for testing. It uses [`async_channel`] internally
///
pub struct MemoryTransport {
    /// Flag to know if it's already connected to the othe half.
    ///
    /// It uses an [`AtomicBool`] to meet the requirements of the [`Transport`] trait
    ///
    connected: AtomicBool,
    /// The sender half of an [`async_channel::bounded`] channel
    ///
    /// It uses an [`async_channel`] to meet the requirements of the [`Transport`] trait
    ///
    /// eg: [`crate::client::RpcClient`] stores the sender half of the channel which the [`crate::server::RpcServer`] stores the receiver half and viceversa
    sender: Sender<Vec<u8>>,
    /// The receiver half of an [`async_channel::bounded`] channel
    ///
    /// It uses an [`async_channel`] to meet the requirements of the [`Transport`] trait
    ///
    /// eg: [`crate::client::RpcClient`] stores the receiver half of the channel which the [`crate::server::RpcServer`] stores the sender half and viceversa
    receiver: Receiver<Vec<u8>>,
}

impl MemoryTransport {
    fn new(sender: Sender<Vec<u8>>, receiver: Receiver<Vec<u8>>) -> Self {
        Self {
            sender,
            receiver,
            connected: AtomicBool::new(false),
        }
    }

    /// It crates two [`MemoryTransport`]s for the both ends
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
    async fn receive(&self) -> Result<TransportEvent, TransportError> {
        match self.receiver.recv().await {
            Ok(event) => {
                let message = self.message_to_transport_event(event);
                if let TransportEvent::Connect = message {
                    self.connected.store(true, Ordering::SeqCst);
                }
                Ok(message)
            }
            Err(_) => {
                self.close().await;
                Ok(TransportEvent::Close)
            }
        }
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        match self.sender.send(message).await {
            Ok(_) => Ok(()),
            Err(_) => Err(TransportError::Connection),
        }
    }

    async fn close(&self) {
        self.receiver.close();
        self.connected.store(false, Ordering::SeqCst);
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}
