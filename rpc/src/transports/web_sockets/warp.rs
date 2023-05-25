use async_trait::async_trait;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Error as TungsteniteError;
use warp::ws::Message as WarpMessage;
use warp::ws::WebSocket as WarpWS;
use warp::Error as WarpError;

use super::Message;
use super::WebSocket;
use super::{convert, Error};

type ReadStream = SplitStream<WarpWS>;
type WriteStream = SplitSink<WarpWS, WarpMessage>;

pub struct WarpWebSocket {
    read: Mutex<ReadStream>,
    write: Mutex<WriteStream>,
}

impl From<WarpMessage> for Message {
    fn from(value: WarpMessage) -> Self {
        if value.is_text() {
            Message::Text(value.to_str().unwrap().to_string())
        } else if value.is_binary() {
            Message::Binary(value.into_bytes())
        } else if value.is_ping() {
            Message::Ping
        } else if value.is_pong() {
            Message::Pong
        } else if value.is_close() {
            Message::Close
        } else {
            unreachable!();
        }
    }
}

impl From<Message> for WarpMessage {
    fn from(value: Message) -> Self {
        match value {
            Message::Text(data) => WarpMessage::text(data),
            Message::Binary(data) => WarpMessage::binary(data),
            Message::Ping => WarpMessage::ping(vec![]),
            Message::Pong => WarpMessage::pong(vec![]),
            Message::Close => WarpMessage::close(),
        }
    }
}

impl From<WarpError> for Error {
    fn from(value: WarpError) -> Self {
        use std::error::Error;
        let source = value.source();
        match source {
            Some(error) => match error.downcast_ref::<TungsteniteError>() {
                Some(TungsteniteError::ConnectionClosed) => Self::ConnectionClosed,
                Some(TungsteniteError::AlreadyClosed) => Self::AlreadyClosed,
                _ => Self::Other(Box::new(value)),
            },
            None => Self::Other(Box::new(value)),
        }
    }
}

#[async_trait]
impl WebSocket for WarpWebSocket {
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

impl WarpWebSocket {
    pub fn new(ws: WarpWS) -> Self {
        let (write, read) = ws.split();
        let (write, read) = (Mutex::new(write), Mutex::new(read));
        Self { write, read }
    }
}
