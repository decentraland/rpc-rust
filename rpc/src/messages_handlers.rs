use std::{collections::HashMap, sync::Arc};

use log::{debug, error};
use prost::Message;
use tokio::{
    select,
    sync::{
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
    service_module_definition::{
        BiStreamsResponse, ClientStreamsResponse, ServerStreamsResponse, UnaryResponse,
    },
    stream_protocol::Generator,
    transports::{Transport, TransportEvent},
};

/// It's in charge of handling every request that the client sends
///
/// It spawns a background tasks to process every request
///
#[derive(Default)]
pub struct ServerMessagesHandler {
    /// Handler for server and client streams procedures
    pub streams_handler: Arc<StreamsHandler>,
    /// Stores listeners for client streams messages
    listeners: Mutex<HashMap<u32, AsyncChannelSender<StreamPackage>>>,
}

impl ServerMessagesHandler {
    pub fn new() -> Self {
        Self {
            streams_handler: Arc::new(StreamsHandler::new()),
            listeners: Mutex::new(HashMap::new()),
        }
    }

    /// Receive a unary procedure handler returned future and process it in a spawned background task.
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    /// # Arguments
    ///
    /// * `transport` - Cloned transport from `RpcServer`
    /// * `message_identifier` - Message id to be sent in the response
    /// * `procedure_handler` - Procedure handler returned future to be executed (awaited)
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

    /// Receive a server streams procedure handler returned future and process it in a spawned background task.
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    /// # Arguments
    ///
    /// * `self: Arc<Self>` - an Arc<Self> that it's a cloned instance that the [`crate::server::RpcServer`] contains. It spawns a background task and it needs to modify its state
    /// * `transport` - Cloned transport from `RpcServer`
    /// * `message_identifier` - Message id to be sent in the response
    /// * `procedure_handler` - Procedure handler returned future to be executed (awaited)
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

            self.streams_handler
                .send_stream_through_transport(transport, stream, port_id, message_identifier)
                .await
                .unwrap()
        });
    }

    pub fn process_client_streams_request(
        self: Arc<Self>,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        client_stream_id: u32,
        procedure_handler: ClientStreamsResponse,
        listener: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        tokio::spawn(async move {
            self.register_listener(client_stream_id, listener).await;
            let response = procedure_handler.await;
            self.send_response(transport, message_identifier, response)
                .await;
        });
    }

    pub fn process_bidir_streams_request(
        self: Arc<Self>,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        port_id: u32,
        client_stream_id: u32,
        listener: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
        procedure_handler: BiStreamsResponse,
    ) {
        tokio::spawn(async move {
            self.register_listener(client_stream_id, listener).await;
            let open_ack_listener = self
                .open_server_stream(transport.clone(), message_identifier, port_id)
                .await
                .unwrap();

            open_ack_listener.await.unwrap();

            let stream = procedure_handler.await;

            self.streams_handler
                .send_stream_through_transport(transport, stream, port_id, message_identifier)
                .await
                .unwrap()
        });
    }

    pub fn notify_new_client_stream(self: Arc<Self>, message_identifier: u32, payload: Vec<u8>) {
        tokio::spawn(async move {
            let lock = self.listeners.lock().await;
            let listener = lock.get(&message_identifier);
            if let Some(listener) = listener {
                listener
                    .send((
                        RpcMessageTypes::StreamMessage,
                        message_identifier,
                        StreamMessage::decode(payload.as_slice()).unwrap(),
                    ))
                    .await
                    .unwrap()
            }
        });
    }

    pub async fn send_response(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        message_identifier: u32,
        payload: Vec<u8>,
    ) {
        let response = Response {
            message_identifier: build_message_identifier(
                RpcMessageTypes::Response as u32,
                message_identifier,
            ),
            payload,
        };

        transport.send(response.encode_to_vec()).await.unwrap();
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

        self.streams_handler
            .send_stream(transport, opening_message)
            .await
    }

    pub async fn register_listener(
        &self,
        message_id: u32,
        callback: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        let mut lock = self.listeners.lock().await;
        lock.insert(message_id, callback);
    }

    pub async fn unregister_listener(&self, message_id: u32) {
        let mut lock = self.listeners.lock().await;
        lock.remove(&message_id);
    }
}

type StreamPackage = (RpcMessageTypes, u32, StreamMessage);

/// `ClientMessagesHandler` is in charge of sending message through the transport, processing the responses and sending them through their attached listeners
///
/// It runs a background task listening for new messages (responses) in the given transport.
///
/// It's the data structure that actually owns the Transport attached to a `RpcClient`. The transport is drilled down up to get to `ClientMEssagesHandler`
///
///
pub struct ClientMessagesHandler {
    /// Transport received by a `RpcClient`
    pub transport: Arc<dyn Transport + Send + Sync>,
    /// Data structure in charge of handling all messages related to streams
    pub streams_handler: Arc<StreamsHandler>,
    /// One time listeners for responses.
    ///
    /// The listeners here are removed once the transport receives the response for their message id
    ///
    /// The raw response (`Vec<u8>`) is sent through the listener
    ///
    /// - key : Message id assigned to a request. A response is returned with the same message id
    /// - value : A oneshot sender from a oneshot channel. It expects the raw response body (`Vec<u8>`) and the function awaiting to receive this is in chage of decoding the raw response body
    ///
    one_time_listeners: Mutex<HashMap<u32, OneShotSender<Vec<u8>>>>,
    /// Listeners for streams.
    ///
    /// A listeners is called every time that the transport receives a response with the listener's message id
    ///
    /// The raw response (`Vec<u8>`) is sent through the listener
    ///
    /// - key : Message id assigned to a stream request
    /// - value : An `async_channel` sender from `async_channel` channel. It expects the raw response body (`Vec<u8>`) and a `StreamProtocol` instance awaiting to receive this is in chage of decoding the raw response body
    ///
    listeners: Mutex<HashMap<u32, AsyncChannelSender<StreamPackage>>>,
    /// Process cancellation token is used for cancelling the background task spawned with `ClientMessagesHandler::start(self: Arc<Self>)`
    ///
    /// If the cancellation token is never triggered, the background task cotinues until the `RpcClient` owning this is dropped
    ///
    process_cancellation_token: CancellationToken,
}

impl ClientMessagesHandler {
    pub fn new(transport: Arc<dyn Transport + Send + Sync>) -> Self {
        Self {
            transport,
            one_time_listeners: Mutex::new(HashMap::new()),
            process_cancellation_token: CancellationToken::new(),
            listeners: Mutex::new(HashMap::new()),
            streams_handler: Arc::new(StreamsHandler::new()),
        }
    }

    /// Starts a background task for listening responses in the transport coping (Arc) an existing instance
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

    /// Stops the background task listening responses in the transport
    pub fn stop(&self) {
        self.process_cancellation_token.cancel();
    }

    /// In charge of looping in the transport wating for new responses and sending the response through a listener
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
                                    if let Err(error) = listener
                                        .send((
                                            message_header.0,
                                            message_header.1,
                                            StreamMessage::decode(data.as_slice()).unwrap(),
                                        ))
                                        .await
                                    {
                                        debug!(
                                            "> Client > Error while sending message {}",
                                            error.to_string()
                                        );
                                    }
                                } else {
                                    self.streams_handler
                                        .clone()
                                        .message_acknowledged_by_peer(message_header.1, data)
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

    pub fn await_server_ack_open_and_send_streams<M: Message + 'static>(
        self: Arc<Self>,
        open_promise: OneShotReceiver<Vec<u8>>,
        client_stream: Generator<M>,
        port_id: u32,
        client_message_id: u32,
    ) {
        let transport = self.transport.clone();
        tokio::spawn(async move {
            let encoded_response = open_promise.await.unwrap();
            let stream_message = StreamMessage::decode(encoded_response.as_slice()).unwrap();

            if stream_message.closed {
                return;
            }

            let new_generator =
                Generator::from_generator(client_stream, |item| item.encode_to_vec());

            self.streams_handler
                .send_stream_through_transport(transport, new_generator, port_id, client_message_id)
                .await
                .unwrap();
        });
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

    pub async fn unregister_listener(&self, message_id: u32) {
        let mut lock = self.listeners.lock().await;
        lock.remove(&message_id);
    }
}

#[derive(Default)]
pub struct StreamsHandler {
    ack_listeners: Mutex<HashMap<String, OneShotSender<Vec<u8>>>>,
}

impl StreamsHandler {
    pub fn new() -> Self {
        Self {
            ack_listeners: Mutex::new(HashMap::new()),
        }
    }

    async fn close_stream(
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

    pub async fn send_stream_through_transport(
        &self,
        transport: Arc<dyn Transport + Send + Sync>,
        mut stream: Generator<Vec<u8>>,
        port_id: u32,
        message_identifier: u32,
    ) -> ServerResult<()> {
        let mut sequence_number = 0;

        while let Some(message) = stream.next().await {
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

        self.close_stream(transport, sequence_number, message_identifier, port_id)
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

    pub fn message_acknowledged_by_peer(
        self: Arc<Self>,
        message_identifier: u32,
        payload: Vec<u8>,
    ) {
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
                    debug!("> Streams Handler > ack listener not found")
                }
            }
        });
    }
}
