use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt, TryStreamExt,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_tungstenite::{accept_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::{Transport, TransportError, TransportEvent};
use async_trait::async_trait;
use log::{debug, error};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_tungstenite::connect_async;

type WriteStream =
    SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>;

type ReadStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebSocketTransport {
    read: Mutex<ReadStream>,
    write: Mutex<WriteStream>,
    ready: AtomicBool,
}

impl WebSocketTransport {
    fn create(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        let (write, read) = ws.split();
        Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
            ready: AtomicBool::new(false),
        }
    }

    pub async fn connect(address: &str) -> Result<WebSocketTransport, TransportError> {
        let (ws, _) = connect_async(address).await?;
        debug!("Connected to {}", address);

        Ok(Self::create(ws))
    }

    pub async fn listen(address: &str) -> Result<WebSocketTransport, TransportError> {
        // listen to given address
        let listener = TcpListener::bind(address).await?;
        debug!("Listening on: {}", address);

        // wait for a connection
        match listener.accept().await {
            Ok((stream, _)) => {
                let peer = stream.peer_addr()?;

                debug!("Peer address: {}", peer);
                let stream = MaybeTlsStream::Plain(stream);
                let ws = accept_async(stream).await?;
                Ok(Self::create(ws))
            }
            _ => Err(TransportError::Connection),
        }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn receive(&self) -> Result<TransportEvent, TransportError> {
        match self.read.lock().await.try_next().await {
            Ok(Some(message)) => {
                if message.is_binary() {
                    let message = self.message_to_transport_event(message.into_data());
                    if let TransportEvent::Connect = message {
                        self.ready.store(true, Ordering::SeqCst);
                    }
                    return Ok(message);
                } else {
                    // Ignore messages that are not binary
                    error!("Received message is not binary");
                    return Err(TransportError::Internal);
                }
            }
            Ok(_) => {
                debug!("Nothing yet")
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
        let message = Message::binary(message);
        self.write.lock().await.send(message).await?;
        Ok(())
    }

    async fn close(&self) {
        match self.write.lock().await.close().await {
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
