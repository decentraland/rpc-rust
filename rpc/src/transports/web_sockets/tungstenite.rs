use std::sync::Arc;

use async_trait::async_trait;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use log::debug;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver},
        Mutex,
    },
    task::JoinHandle,
};
use tokio_tungstenite::{
    accept_async, connect_async,
    tungstenite::{Error as TungsteniteError, Message as TungsteniteMessage},
    MaybeTlsStream, WebSocketStream,
};

use crate::transports::TransportError;

use super::{convert, Error, Message, WebSocket};

/// Write Stream Half of [`WebSocketStream`]
type WriteStream =
    SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Message>;

/// Read Stream Half of [`WebSocketStream`]
type ReadStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// A [`WebSocketStream`] from a WebSocket connection
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct TungsteniteWebSocket {
    read: Mutex<ReadStream>,
    write: Mutex<WriteStream>,
}

impl TungsteniteWebSocket {
    pub fn new(ws: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
        let (write, read) = ws.split();
        Self {
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }
}

impl From<TungsteniteError> for Error {
    fn from(value: TungsteniteError) -> Self {
        match value {
            TungsteniteError::ConnectionClosed => Error::ConnectionClosed,
            TungsteniteError::AlreadyClosed => Error::AlreadyClosed,
            _ => Error::Other(Box::new(value)),
        }
    }
}

impl From<Message> for TungsteniteMessage {
    fn from(value: Message) -> Self {
        match value {
            Message::Text(data) => TungsteniteMessage::Text(data),
            Message::Binary(data) => TungsteniteMessage::Binary(data),
            Message::Ping => TungsteniteMessage::Ping(vec![]),
            Message::Pong => TungsteniteMessage::Pong(vec![]),
            Message::Close => TungsteniteMessage::Close(None),
        }
    }
}

impl From<TungsteniteMessage> for Message {
    fn from(value: TungsteniteMessage) -> Self {
        match value {
            TungsteniteMessage::Text(data) => Message::Text(data),
            TungsteniteMessage::Binary(data) => Message::Binary(data),
            TungsteniteMessage::Ping(_) => Message::Ping,
            TungsteniteMessage::Pong(_) => Message::Pong,
            TungsteniteMessage::Close(_) => Message::Close,
            TungsteniteMessage::Frame(_) => unreachable!(),
        }
    }
}

#[async_trait]
impl WebSocket for TungsteniteWebSocket {
    async fn send(&self, message: Message) -> Result<(), Error> {
        Ok(self.write.lock().await.send(message.into()).await?)
    }

    async fn receive(&self) -> Option<Result<Message, Error>> {
        self.read.lock().await.next().await.map(convert)
    }

    async fn close(&self) -> Result<(), Error> {
        Ok(self.write.lock().await.close().await?)
    }
}

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
type OnConnectionListener =
    UnboundedReceiver<Result<Socket, Box<dyn std::error::Error + Send + Sync>>>;

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
            unbounded_channel::<Result<Socket, Box<dyn std::error::Error + Send + Sync>>>();

        let join_handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let peer = match stream.peer_addr() {
                            Ok(peer) => peer,
                            Err(err) => {
                                debug!(
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
                                    debug!(
                                            "> WS Server > Error on sending the new ws socket to listener"
                                    );
                                    break;
                                }
                            }
                            Err(error) => {
                                debug!("> WS Server > Error on upgrading the socket: {error:?}");
                                if tx_on_connection_listener
                                    .send(Err(Box::new(error)))
                                    .is_err()
                                {
                                    debug!(
                                        "> WS Server > Error on sending an error to the listener"
                                    );
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                    Err(error) => {
                        debug!("> WS Server > Error on accepting a stream {error:?}");
                        if tx_on_connection_listener
                            .send(Err(Box::new(error)))
                            .is_err()
                        {
                            debug!("> WS Server > Error on sending the error to the listener")
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

pub struct WebSocketClient;

impl WebSocketClient {
    pub async fn connect(host: &str) -> Result<Arc<TungsteniteWebSocket>, TransportError> {
        let (websocket_stream, _) = connect_async(host).await?;
        debug!("Connected to {}", host);

        let websocket = Arc::new(TungsteniteWebSocket::new(websocket_stream));
        Ok(websocket)
    }
}
