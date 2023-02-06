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
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver},
        Mutex,
    },
};
use tokio_tungstenite::connect_async;

type WriteStream =
    SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>;

type ReadStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WebSocketServer to receive connections
pub struct WebSocketServer {
    /// Address to listen for new connection
    address: String,
}

/// Receiver half of a channel to get notified that there is a new connection
///
/// And then attach turn the connection into a transport and attach it to the `RpcServer`
///
type OnConnectionListener = UnboundedReceiver<Result<Socket, TransportError>>;

impl WebSocketServer {
    /// Set the configuration and the minimum for a new WebSocket Server
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
        }
    }

    /// Listen for new connection on the address given in a background task and send the new connection through the listener in order to handle it.
    pub async fn listen(&self) -> Result<OnConnectionListener, TransportError> {
        // listen to given address
        let listener = TcpListener::bind(&self.address).await?;
        debug!("Listening on: {}", self.address);

        let (tx_on_connection_listener, rx_on_connection_listener) = unbounded_channel();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let peer = if let Ok(perr) = stream.peer_addr() {
                            perr
                        } else {
                            if tx_on_connection_listener
                                .send(Err(TransportError::Connection))
                                .is_err()
                            {
                                error!("WS Server: Error on sending the error to the listener")
                            }
                            continue;
                        };

                        debug!("Peer address: {}", peer);
                        let stream = MaybeTlsStream::Plain(stream);
                        if let Ok(ws) = accept_async(stream).await {
                            if tx_on_connection_listener.send(Ok(ws)).is_err() {
                                error!("WS Server: Error on sending the new ws socket to listener")
                            }
                        } else {
                            if tx_on_connection_listener
                                .send(Err(TransportError::Connection))
                                .is_err()
                            {
                                error!("WS Server: Error on sending the error to the listener")
                            }
                            continue;
                        };
                    }
                    Err(error) => {
                        if tx_on_connection_listener
                            .send(Err(TransportError::Connection))
                            .is_err()
                        {
                            error!(
                                "WS Server: Error on sending the error to the listener: {error:?}"
                            )
                        }
                    }
                }
            }
        });

        Ok(rx_on_connection_listener)
    }
}

/// WebScoketClient structure to connect to a WebSocket Server
pub struct WebSocketClient;

impl WebSocketClient {
    /// Connect to a websocket server
    pub async fn connect(
        host: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, TransportError> {
        let (ws, _) = connect_async(host).await?;
        debug!("Connected to {}", host);
        Ok(ws)
    }
}

pub struct WebSocketTransport {
    read: Mutex<ReadStream>,
    write: Mutex<WriteStream>,
    ready: AtomicBool,
}

impl WebSocketTransport {
    pub fn new(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        let (write, read) = ws.split();
        Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
            ready: AtomicBool::new(false),
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
