//! Websockets as the wire between an [`RpcServer`](crate::server::RpcServer) and a [`RpcClient`](crate::client::RpcClient).
//!
//! This let the user get the most out of the advantages of using Decentraland RPC.
use super::{Transport, TransportError, TransportMessage};
use async_trait::async_trait;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use log::{debug, error};
use std::{error::Error, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver},
        Mutex,
    },
    task::JoinHandle,
    time::interval,
};
use tokio_tungstenite::{
    accept_async, connect_async,
    tungstenite::{Error as TungsteniteError, Message},
    MaybeTlsStream, WebSocketStream,
};

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
type OnConnectionListener = UnboundedReceiver<Result<Socket, Box<dyn Error + Send + Sync>>>;

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
    pub async fn listen(&mut self) -> Result<OnConnectionListener, std::io::Error> {
        // listen to given address
        let listener = TcpListener::bind(&self.address).await?;
        debug!("Listening on: {}", self.address);

        let (tx_on_connection_listener, rx_on_connection_listener) =
            unbounded_channel::<Result<Socket, Box<dyn Error + Send + Sync>>>();

        let join_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let peer = match stream.peer_addr() {
                            Ok(peer) => peer,
                            Err(err) => {
                                error!(
                                    "> WS Server > Error on get the remote address of the socket: {err:?}"
                                );
                                continue;
                            }
                        };

                        debug!("> WS Server > Peer address: {}", peer);
                        let stream = MaybeTlsStream::Plain(stream);
                        match accept_async(stream).await {
                            Ok(ws) => {
                                if tx_on_connection_listener.send(Ok(ws)).is_err() {
                                    error!(
                                            "> WS Server > Error on sending the new ws socket to listener"
                                    );
                                    break;
                                }
                            }
                            Err(error) => {
                                error!("> WS Server > Error on upgrading the socket: {error:?}");
                                if tx_on_connection_listener
                                    .send(Err(Box::new(error)))
                                    .is_err()
                                {
                                    error!(
                                        "> WS Server > Error on sending an error to the listener"
                                    );
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                    Err(error) => {
                        error!("> WS Server > Error on accepting a stream {error:?}");
                        if tx_on_connection_listener
                            .send(Err(Box::new(error)))
                            .is_err()
                        {
                            error!("> WS Server > Error on sending the error to the listener")
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

pub struct WebSocket {
    read: Mutex<ReadStream>,
    write: Mutex<WriteStream>,
}

impl WebSocket {
    pub fn new(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        let (write, read) = ws.split();
        Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }

    async fn send(&self, message: Message) -> Result<(), TungsteniteError> {
        self.write.lock().await.send(message).await
    }

    async fn receive(&self) -> Option<Result<Message, TungsteniteError>> {
        self.read.lock().await.next().await
    }

    async fn close(&self) -> Result<(), TungsteniteError> {
        self.write.lock().await.close().await
    }
}
/// WebScoketClient structure to connect to a WebSocket Server
pub struct WebSocketClient;

impl WebSocketClient {
    /// Connect to a websocket server and returns a [`WebSocketStream`] if all went OK or a [`TransportError`] if there was a error on establishing the connection
    ///
    /// If all went OK, and the [`WebSocketStream`] is returned, it should be turned into a [`WebSocketTransport`] to be attached to the [`RpcClient`](crate::client::RpcClient)
    ///
    pub async fn connect(host: &str) -> Result<Arc<WebSocket>, TransportError> {
        let (websocket_stream, _) = connect_async(host).await?;
        debug!("Connected to {}", host);

        let websocket = Arc::new(WebSocket::new(websocket_stream));
        let websocket_ping = websocket.clone();
        tokio::spawn(async move {
            let mut ping_interval = interval(Duration::from_secs(30));
            loop {
                ping_interval.tick().await;
                _ = websocket_ping.send(Message::Ping(vec![])).await;
            }
        });

        Ok(websocket)
    }
}

/// Transport to be used when there is a websocket server listening for new connections.
///
/// Each new connection received from [`WebSocketServer`] should be passed to a [`WebSocketTransport`](#method.WebSocketTransport.new) and then the new [`WebSocketTransport`] should be attached to the [`RpcServer`](crate::server::RpcServer)
///
/// Or a each new connection to a websocket server through [`WebSocketClient`] should be passed to a [`WebSocketTransport`](#method.WebSocketTransport.new) a passed to [`RpcClient`](crate::client::RpcClient) constructor
///
pub struct WebSocketTransport {
    websocket: Arc<WebSocket>,
}

impl WebSocketTransport {
    /// Crates a new [`WebSocketTransport`] from a websocket connection generated by [`WebSocketServer`] or [`WebSocketClient`]
    pub fn new(websocket: Arc<WebSocket>) -> Self {
        Self { websocket }
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn receive(&self) -> Result<TransportMessage, TransportError> {
        loop {
            match self.websocket.receive().await {
                Some(Ok(message)) => match message {
                    Message::Binary(data) => return Ok(data),
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => return Err(TransportError::Closed),
                    _ => return Err(TransportError::NotBinaryMessage),
                },
                Some(Err(err)) => {
                    error!(
                        "> WebSocketTransport > Failed to receive message {}",
                        err.to_string()
                    );
                    match err {
                        TungsteniteError::ConnectionClosed | TungsteniteError::AlreadyClosed => {
                            return Err(TransportError::Closed)
                        }
                        error => return Err(TransportError::Internal(Box::new(error))),
                    }
                }
                None => {
                    error!("> WebSocketTransport > None received > Closing...");
                    return Err(TransportError::Closed);
                }
            }
        }
    }

    async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        let message = Message::binary(message);
        match self.websocket.send(message).await {
            Err(err) => {
                error!(
                    "> WebSocketTransport > Error on sending in a ws connection {}",
                    err.to_string()
                );

                let error = match err {
                    TungsteniteError::ConnectionClosed | TungsteniteError::AlreadyClosed => {
                        TransportError::Closed
                    }
                    error => TransportError::Internal(Box::new(error)),
                };

                Err(error)
            }
            Ok(_) => Ok(()),
        }
    }

    async fn close(&self) {
        match self.websocket.close().await {
            Ok(_) => {
                debug!("> WebSocketTransport > Closed successfully")
            }
            Err(err) => {
                error!("> WebSocketTransport > Error: Couldn't close tranport: {err:?}")
            }
        }
    }
}
