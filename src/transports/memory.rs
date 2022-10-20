use async_trait::async_trait;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use super::{Transport, TransportError, TransportEvent};

pub struct MemoryTransport {
    connected: bool,
    sender: Sender<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
}

impl MemoryTransport {
    fn new(sender: Sender<Vec<u8>>, receiver: Receiver<Vec<u8>>) -> Self {
        Self {
            sender,
            receiver,
            connected: false,
        }
    }

    pub fn create() -> (Self, Self) {
        let (client_sender, server_receiver) = channel::<Vec<u8>>(32);
        let (server_sender, client_receiver) = channel::<Vec<u8>>(32);

        let client = Self::new(client_sender, client_receiver);
        let server = Self::new(server_sender, server_receiver);

        (client, server)
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn receive(&mut self) -> Result<TransportEvent, TransportError> {
        match self.receiver.recv().await {
            Some(event) => {
                if event.len() == 1 && event[0] == 0 {
                    self.connected = true;
                    return Ok(TransportEvent::Connect);
                }
                Ok(TransportEvent::Message { data: event })
            }
            None => Ok(TransportEvent::Close),
        }
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        match self.sender.send(message).await {
            Ok(_) => Ok(()),
            Err(_) => Err(TransportError::Connection),
        }
    }

    fn close(&mut self) {
        self.receiver.close()
    }
}
