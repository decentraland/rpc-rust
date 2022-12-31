use async_channel::{bounded, Receiver, Sender};
use async_trait::async_trait;

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
                if event.len() == 1 && event[0] == 0 {
                    return Ok(TransportEvent::Connect);
                }
                Ok(TransportEvent::Message(event))
            }
            Err(_) => {
                self.receiver.close();
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
    }

    fn establish_connection(&mut self) {
        self.connected = true;
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
