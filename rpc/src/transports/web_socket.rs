//! Websockets as the wire between an [`RpcServer`](crate::server::RpcServer) and a [`RpcClient`](crate::client::RpcClient).
//!
//! This let the user get the most out of the advantages of using Decentraland RPC.
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
    task::JoinHandle,
};
use tokio_tungstenite::connect_async;

/// Write Stream Half of [`WebSocketStream`]
type WriteStream =
    SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>;

/// Read Stream Half of [`WebSocketStream`]
type ReadStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// A [`WebSocketStream`] from a WebSocket connection
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// WebSocketServer using [`tokio_tungstenite`] to receive connections
///
/// You can use another websocket server as long as it meets the interface requirements
///
pub struct WebSocketServer {
    /// Address to listen for new connection
    address: String,
    /// TPC Listener Join Handle
    tpc_listener_handle: Option<JoinHandle<()>>,
}

/// Receiver half of a channel to get notified that there is a new connection
///
/// And then attach turn the connection into a transport and attach it to the [`RpcServer`](crate::server::RpcServer)
///
type OnConnectionListener = UnboundedReceiver<Result<Socket, TransportError>>;

impl WebSocketServer {
    /// Set the configuration and the minimum for a new WebSocket Server
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_string(),
            tpc_listener_handle: None,
        }
    }

    /// Listen for new connections on the address given and do the websocket handshake in a background task
    ///
    /// Each new connection will be sent through the `OnConnectionListener`, in order to be attached to the [`RpcServer`](crate::server::RpcServer)  as a [`WebSocketTransport`]
    ///
    pub async fn listen(&mut self) -> Result<OnConnectionListener, TransportError> {
        // listen to given address
        let listener = TcpListener::bind(&self.address).await?;
        debug!("Listening on: {}", self.address);

        let (tx_on_connection_listener, rx_on_connection_listener) = unbounded_channel();

        let join_handle = tokio::spawn(async move {
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

        self.tpc_listener_handle = Some(join_handle);

        Ok(rx_on_connection_listener)
    }
}

impl Drop for WebSocketServer {
    fn drop(&mut self) {
        if let Some(handle) = &self.tpc_listener_handle {
            handle.abort();
        }
    }
}

/// WebScoketClient structure to connect to a WebSocket Server
pub struct WebSocketClient;

impl WebSocketClient {
    /// Connect to a websocket server and returns a [`WebSocketStream`] if all went OK or a [`TransportError`] if there was a error on establishing the connection
    ///
    /// If all went OK, and the [`WebSocketStream`] is returned, it should be turned into a [`WebSocketTransport`] to be attached to the [`RpcClient`](crate::client::RpcClient)
    ///
    pub async fn connect(
        host: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, TransportError> {
        let (ws, _) = connect_async(host).await?;
        debug!("Connected to {}", host);
        Ok(ws)
    }
}

/// Transport to be used when there is a websocket server listening for new connections.
///
/// Each new connection received from [`WebSocketServer`] should be passed to a [`WebSocketTransport`](#method.WebSocketTransport.new) and then the new [`WebSocketTransport`] should be attached to the [`RpcServer`](crate::server::RpcServer)
///
/// Or a each new connection to a websocket server through [`WebSocketClient`] should be passed to a [`WebSocketTransport`](#method.WebSocketTransport.new) a passed to [`RpcClient`](crate::client::RpcClient) constructor
///
pub struct WebSocketTransport {
    /// The read stream half of the [`WebSocketStream`]
    ///
    /// It's inside a [`Mutex`] in order to meet the requirements of the [`Transport`] trait that doesn't have mutable methods so we should do _Interior Mutability_
    ///
    read: Mutex<ReadStream>,
    /// The write stream half of the [`WebSocketStream`]
    ///
    /// It's inside a [`Mutex`] in order to meet the requirements of the [`Transport`] trait that doesn't have mutable methods so we should do _Interior Mutability_
    ///
    write: Mutex<WriteStream>,
    /// Field to know if the socket is ready to start communicating with the other half
    ///
    /// It's an [`AtomicBool`] in order to meet the requirements of the [`Transport`] trait that doesn't have mutable methods so we should do _Interior Mutability_
    ///
    ready: AtomicBool,
}

impl WebSocketTransport {
    /// Crates a new [`WebSocketTransport`] from a websocket connection generated by [`WebSocketServer`] or [`WebSocketClient`]
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
                    error!("> WebSocketTransport > Received message is not binary");
                    return Err(TransportError::Internal);
                }
            }
            Ok(_) => {
                debug!("> WebSocketTransport > Nothing yet")
            }
            Err(err) => {
                error!(
                    "> WebSocketTransport > Failed to receive message {}",
                    err.to_string()
                );
            }
        }
        debug!("> WebSocketTransport > Closing transport...");
        self.close().await;
        Ok(TransportEvent::Close)
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        let message = Message::binary(message);
        match self.write.lock().await.send(message).await {
            Err(err) => {
                error!(
                    "> WebSocketTransport > Error on sending in a ws connection {}",
                    err.to_string()
                );

                use tokio_tungstenite::tungstenite::Error::*;

                let error = match err {
                    ConnectionClosed | AlreadyClosed => TransportError::Closed,
                    _ => TransportError::Internal,
                };

                Err(error)
            }
            Ok(_) => Ok(()),
        }
    }

    async fn close(&self) {
        if self.is_connected() {
            match self.write.lock().await.close().await {
                Ok(_) => {
                    self.ready.store(false, Ordering::SeqCst);
                }
                _ => {
                    debug!("> WebSocketTransport > Couldn't close tranport")
                }
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}
