use std::{collections::HashMap, sync::Arc};

use log::{debug, error};
use prost::Message;
use tokio::{
    select,
    sync::{
        mpsc::UnboundedReceiver,
        oneshot::{
            channel as oneshot_channel, Receiver as OneShotReceiver, Sender as OneShotSender,
        },
        Mutex,
    },
};

use async_channel::Sender as AsyncChannelSender;
use tokio_util::sync::CancellationToken;

use crate::{
    protocol::{
        parse::{
            build_message_identifier, parse_header, parse_message_identifier,
            parse_protocol_message,
        },
        Response, RpcMessageTypes, StreamMessage,
    },
    server::{ServerError, ServerResult},
    transports::{Transport, TransportEvent},
    types::{ServerStreamsResponse, UnaryResponse},
};

pub struct ServerMessagesHandler {
    ack_listeners: Mutex<HashMap<String, OneShotSender<Vec<u8>>>>,
}

impl ServerMessagesHandler {
    pub fn new() -> Self {
        Self {
            ack_listeners: Mutex::new(HashMap::new()),
        }
    }

    /// Receive a procedure handler future and process it in another task.
    ///
    /// This function aims to run the procedure handler in another task to achieve processing requests concurrently.
    /// # Arguments
    ///
    /// * `transport` - Cloned transport from `RpcServer`
    /// * `message_idenifier` - Message id to be sent in the response
    /// * `request_handler` - Procedure handler future to be executed
    pub fn process_unary_request(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        procedure_handler: UnaryResponse,
    ) {
        tokio::spawn(async move {
            let procedure_response = procedure_handler.await;
            let response = Response {
                message_identifier: build_message_identifier(
                    RpcMessageTypes::Response as u32,
                    message_identifier,
                ),
                payload: procedure_response,
            };

            transport.send(response.encode_to_vec()).await.unwrap();
        });
    }

    pub fn process_server_streams_request(
        self: Arc<Self>,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        port_id: u32,
        procedure_handler: ServerStreamsResponse,
    ) {
        tokio::spawn(async move {
            let open_ack_listener = self
                .open_server_stream(transport.clone(), message_identifier, port_id)
                .await
                .unwrap();

            open_ack_listener.await.unwrap();

            let stream = procedure_handler.await;

            self.send_server_stream_through_transport(
                transport,
                stream,
                port_id,
                message_identifier,
            )
            .await
            .unwrap()
        });
    }

    async fn open_server_stream(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        port_id: u32,
    ) -> ServerResult<OneShotReceiver<Vec<u8>>> {
        let opening_message = StreamMessage {
            closed: false,
            ack: false,
            sequence_id: 0,
            message_identifier: build_message_identifier(
                RpcMessageTypes::StreamMessage as u32,
                message_identifier,
            ),
            port_id,
            payload: vec![],
        };

        self.send_stream(transport, opening_message).await
    }

    async fn close_server_stream(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        sequence_id: u32,
        message_identifier: u32,
        port_id: u32,
    ) -> ServerResult<()> {
        let close_message = StreamMessage {
            closed: true,
            ack: false,
            sequence_id,
            message_identifier: build_message_identifier(
                RpcMessageTypes::StreamMessage as u32,
                message_identifier,
            ),
            port_id,
            payload: vec![],
        };

        transport.send(close_message.encode_to_vec()).await.unwrap();

        Ok(())
    }

    async fn send_server_stream_through_transport(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        mut stream: UnboundedReceiver<Vec<u8>>,
        port_id: u32,
        message_identifier: u32,
    ) -> ServerResult<()> {
        let mut sequence_number = 0;

        while let Some(message) = stream.recv().await {
            sequence_number += 1;
            let current_message = StreamMessage {
                closed: false,
                ack: false,
                sequence_id: sequence_number,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::StreamMessage as u32,
                    message_identifier,
                ),
                port_id,
                payload: message,
            };
            let transport_cloned = transport.clone();

            match self.send_stream(transport_cloned, current_message).await {
                Ok(listener) => {
                    let ack_message = match listener.await {
                        Ok(msg) => match StreamMessage::decode(msg.as_slice()) {
                            Ok(msg) => msg,
                            Err(_) => break,
                        },
                        Err(_) => break,
                    };
                    if ack_message.ack {
                        continue;
                    } else if ack_message.closed {
                        break;
                    }
                }
                Err(err) => {
                    error!("Error while streaming a server stream {err:?}");
                    break;
                }
            }
        }

        self.close_server_stream(transport, sequence_number, message_identifier, port_id)
            .await
            .unwrap();

        Ok(())
    }

    async fn send_stream(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        message: StreamMessage,
    ) -> ServerResult<OneShotReceiver<Vec<u8>>> {
        let (_, message_id) = parse_message_identifier(message.message_identifier);
        let (tx, rx) = oneshot_channel();
        {
            let mut lock = self.ack_listeners.lock().await;
            lock.insert(format!("{}{}", message_id, message.sequence_id), tx);
        }

        transport
            .send(message.encode_to_vec())
            .await
            .map_err(|_| ServerError::TransportError)?;

        Ok(rx)
    }

    pub fn acknowledge_message(self: Arc<Self>, message_identifier: u32, payload: Vec<u8>) {
        tokio::spawn(async move {
            let stream_message = parse_protocol_message::<StreamMessage>(&payload).unwrap().2;
            let listener = {
                let mut lock = self.ack_listeners.lock().await;
                // we should remove ack listener it just for a seq_id
                lock.remove(&format!(
                    "{}{}",
                    message_identifier, stream_message.sequence_id
                ))
            };
            match listener {
                Some(sender) => sender.send(payload).unwrap(),
                None => {
                    debug!("> RpcServer > ack listener not found")
                }
            }
        });
    }
}

pub struct ClientMessagesHandler {
    pub transport: Arc<dyn Transport + Send + Sync>,
    one_time_listeners: Mutex<HashMap<u32, OneShotSender<Vec<u8>>>>,
    listeners: Mutex<HashMap<u32, AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>>>,
    process_cancellation_token: CancellationToken,
}

impl ClientMessagesHandler {
    pub fn new(transport: Arc<dyn Transport + Send + Sync>) -> Self {
        Self {
            transport,
            one_time_listeners: Mutex::new(HashMap::new()),
            process_cancellation_token: CancellationToken::new(),
            listeners: Mutex::new(HashMap::new()),
        }
    }

    pub fn start(self: Arc<Self>) {
        let token = self.process_cancellation_token.clone();
        tokio::spawn(async move {
            select! {
                _ = token.cancelled() => {
                    debug!("> ClientRequestDispatcher cancelled!")
                },
                _ = self.process() => {

                }
            }
        });
    }

    pub fn stop(&self) {
        self.process_cancellation_token.cancel();
    }

    async fn process(&self) {
        loop {
            match self.transport.receive().await {
                Ok(TransportEvent::Message(data)) => {
                    let message_header = parse_header(&data);
                    // TODO: find a way to communicate the error of parsing the message_header
                    match message_header {
                        Some(message_header) => {
                            let mut read_callbacks = self.one_time_listeners.lock().await;
                            // We remove the listener in order to get a owned value and also remove it from memory
                            let sender = read_callbacks.remove(&message_header.1);
                            if let Some(sender) = sender {
                                match sender.send(data) {
                                    Ok(()) => {}
                                    Err(_) => {
                                        debug!(
                                            "> Client > error while sending {} response",
                                            message_header.1
                                        );
                                        continue;
                                    }
                                }
                            } else {
                                let listeners = self.listeners.lock().await;
                                let listener = listeners.get(&message_header.1);
                                if let Some(listener) = listener {
                                    match listener
                                        .send((
                                            message_header.0,
                                            message_header.1,
                                            StreamMessage::decode(data.as_slice()).unwrap(),
                                        ))
                                        .await
                                    {
                                        Ok(_) => {}
                                        Err(_) => {}
                                    }
                                }
                            }
                        }
                        None => {
                            debug!("> Client > Error on parsing message header");
                            continue;
                        }
                    }
                }
                Ok(_) => {
                    // Ignore another type of TransportEvent
                    continue;
                }
                Err(_) => {
                    error!("Client error on receiving");
                    break;
                }
            }
        }
    }

    pub async fn register_one_time_listener(
        &self,
        message_id: u32,
        callback: OneShotSender<Vec<u8>>,
    ) {
        let mut lock = self.one_time_listeners.lock().await;
        lock.insert(message_id, callback);
    }

    pub async fn register_listener(
        &self,
        message_id: u32,
        callback: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        let mut lock = self.listeners.lock().await;
        lock.insert(message_id, callback);
    }
}
