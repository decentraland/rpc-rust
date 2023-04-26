//! QUIC as the wire between an [`RpcServer`](crate::server::RpcServer)  and a [`RpcClient`](crate::client::RpcClient).
use super::{Transport, TransportError, TransportEvent};
use crate::rpc_protocol::{parse::parse_header, RpcMessageTypes};
use async_trait::async_trait;
use log::{debug, error};
use quinn::{ClientConfig, Endpoint, RecvStream, SendStream, ServerConfig};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex;

pub struct QuicTransport {
    send: Mutex<SendStream>,
    receiver: Mutex<RecvStream>,
    ready: AtomicBool,
}

impl QuicTransport {
    fn new((send, receiver): (SendStream, RecvStream)) -> Self {
        Self {
            send: Mutex::new(send),
            receiver: Mutex::new(receiver),
            ready: AtomicBool::new(false),
        }
    }

    pub async fn create_client(
        client_address: &str,
        server_address: &str,
        server_name: &str,
        config: ClientConfig,
    ) -> Result<Self, TransportError> {
        let server_address = server_address.parse::<SocketAddr>()?;
        let client_address = client_address.parse::<SocketAddr>()?;

        // Bind this endpoint to a UDP socket on the given client address.
        let mut endpoint = Endpoint::client(client_address)?;
        endpoint.set_default_client_config(config);

        // Connect to the server passing in the server name which is supposed to be in the server certificate.
        let connection = endpoint.connect(server_address, server_name)?.await?;

        let bi_stream = connection.open_bi().await?;
        let (mut send, receiver) = bi_stream;

        // Opening a bidirectional stream is cheap, so server will not be notified until we send a
        // message!
        send.write_all(b"hello!").await?;

        Ok(Self::new((send, receiver)))
    }

    pub async fn create_server(
        address: &str,
        config: ServerConfig,
    ) -> Result<Self, TransportError> {
        let address = address.parse::<SocketAddr>()?;

        // Bind this endpoint to a UDP socket on the given server address.
        let endpoint = Endpoint::server(config, address)?;

        let connection = endpoint.accept().await.ok_or(TransportError::Closed)?;
        let bi_stream = connection.await?.accept_bi().await?;
        Ok(Self::new(bi_stream))
    }
}

#[async_trait]
impl Transport for QuicTransport {
    async fn receive(&self) -> Result<TransportEvent, TransportError> {
        // TODO: make this number configurable.
        // `1024` is the max lenght to read in the next datagram.
        match self.receiver.lock().await.read_chunk(1024, true).await {
            Ok(Some(chunk)) => {
                let message = chunk.bytes.to_vec();
                if !self.is_connected() {
                    self.ready.store(true, Ordering::SeqCst);
                    if let Some((message_type, _)) = parse_header(&message) {
                        // This is exclusively for the client
                        if matches!(message_type, RpcMessageTypes::ServerReady) {
                            return Ok(TransportEvent::Connect);
                        }
                    }
                }
                return Ok(TransportEvent::Message(message));
            }
            _ => {
                error!("Failed to receive message");
            }
        }
        debug!("Closing transport...");
        self.close().await;
        Ok(TransportEvent::Close)
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        self.send.lock().await.write_all(&message).await?;
        Ok(())
    }

    async fn close(&self) {
        match self.send.lock().await.finish().await {
            Ok(_) => {
                self.ready.store(false, Ordering::SeqCst);
            }
            _ => {
                debug!("Couldn't close tranport")
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
}
