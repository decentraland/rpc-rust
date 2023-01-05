use futures_util::{
    stream::{SplitSink, SplitStream},
    StreamExt, SinkExt, TryStreamExt,
};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_tungstenite::{accept_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use async_trait::async_trait;

use super::{Transport, TransportError, TransportEvent};
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
        let (ws, _) = connect_async(address)
            .await
            .map_err(|_| TransportError::Connection)?;
        println!("Connected to {}", address);

        Ok(Self::create(ws))
    }

    pub async fn listen(address: &str) -> Result<WebSocketTransport, TransportError> {
        // listen to given address
        let listener = TcpListener::bind(address).await.expect("Can't listen");
        println!("Listening on: {}", address);

        // wait for a connection
        match listener.accept().await {
            Ok((stream, _)) => {
                let peer = stream
                    .peer_addr()
                    .expect("connected streams should have a peer address");
                println!("Peer address: {}", peer);
                let stream = MaybeTlsStream::Plain(stream);
                let ws = accept_async(stream)
                    .await
                    .map_err(|_| TransportError::Internal)?;
                Ok(Self::create(ws))
            }
            _ => Err(TransportError::Connection),
        }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn receive(&self) -> Result<TransportEvent, TransportError> {
        while let Ok(next) = self.read.lock().await.try_next().await {
            match next {
                Some(message) => {
                    if message.is_binary() {
                        let message = message.into_data();
                        if message.len() == 1 && message[0] == 0 && !self.is_connected() {
                            self.ready.store(true, Ordering::SeqCst);
                            return Ok(TransportEvent::Connect);
                        }
                        return Ok(TransportEvent::Message(message));
                    } else {
                        // Ignore messages that are not binary
                        return Err(TransportError::Internal);
                    }
                }
                None => {
                    println!("Nothing yet")
                }
            }
        }
        self.close().await;
        Ok(TransportEvent::Close)
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        let message = Message::binary(message);
        self.write
            .lock()
            .await
            .send(message)
            .await
            .map_err(|_| TransportError::Connection)?;
        Ok(())
    }

    async fn close(&self) {
        match self.write.lock().await.close().await {
            Ok(_) => {
                self.ready.store(false, Ordering::SeqCst);
            }
            _ => {
                println!("Couldn't close tranport")
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
}
