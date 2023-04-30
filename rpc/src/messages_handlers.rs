//! It contains the types that the [`RpcServer`](crate::server::RpcServer)  and [`RpcClient`](crate::client::RpcClient) use to handle messages (requests and reponses).
//!
//! Also, it contains the type [`StreamsHandler`] to handle streams. This type is used by both because they both receive and return StreamMessages
//!
use crate::{
    rpc_protocol::{
        fill_remote_error,
        parse::{
            build_message_identifier, parse_message_identifier, parse_protocol_message, ParseErrors,
        },
        RemoteError, Response, RpcMessageTypes, StreamMessage,
    },
    server::{ServerError, ServerInternalError, ServerResult, ServerResultError},
    service_module_definition::{
        BiStreamsResponse, ClientStreamsResponse, ServerStreamsResponse, UnaryResponse,
    },
    stream_protocol::Generator,
    transports::{Transport, TransportError},
    CommonError,
};
use async_channel::Sender as AsyncChannelSender;
use log::{debug, error};
use prost::Message;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    oneshot::{channel as oneshot_channel, Receiver as OneShotReceiver, Sender as OneShotSender},
    Mutex,
};

/// It's in charge of handling every request that the client sends
///
/// It spawns a background tasks to process every request
///
#[derive(Default)]
#[cfg(feature = "server")]
pub struct ServerMessagesHandler {
    /// Data structure in charge of handling all messages related to streams
    pub streams_handler: Arc<StreamsHandler>,
    /// Stores listeners for client streams messages
    listeners: Mutex<HashMap<u32, AsyncChannelSender<StreamPackage>>>,
}

#[cfg(feature = "server")]
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
    pub fn process_unary_request<T: Transport + ?Sized + 'static>(
        &self,
        transport: Arc<T>,
        message_number: u32,
        procedure_handler: UnaryResponse,
    ) {
        tokio::spawn(async move {
            match procedure_handler.await {
                Ok(procedure_response) => {
                    let response = Response {
                        message_identifier: build_message_identifier(
                            RpcMessageTypes::Response as u32,
                            message_number,
                        ),
                        payload: procedure_response,
                    };

                    if let Err(err) = transport.send(response.encode_to_vec()).await {
                        error!("> ServerMessagesHandler > Error on sending procedure response through a transport - message num: {message_number} - error: {err:?}");
                        if !matches!(err, TransportError::Closed) {
                            // Try to communicate the error to client. Maybe the transport is not closed and it's another type of error
                            send_remote_error(
                                transport,
                                message_number,
                                ServerError::UnexpectedErrorOnTransport.into(),
                            )
                            .await;
                        }
                    }
                }
                Err(procedure_remote_error) => {
                    send_remote_error(transport, message_number, procedure_remote_error).await;
                }
            };
        });
    }

    /// Receive a server streams procedure handler returned future and process it in a spawned background task.
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    pub fn process_server_streams_request<T: Transport + ?Sized + 'static>(
        self: Arc<Self>,
        transport: Arc<T>,
        message_number: u32,
        port_id: u32,
        procedure_handler: ServerStreamsResponse,
    ) {
        tokio::spawn(async move {
            match self
                .open_server_stream(transport.clone(), message_number, port_id)
                .await
            {
                Ok(open_ack_listener) => match open_ack_listener.await {
                    Ok(_) => match procedure_handler.await {
                        Ok(generator) => {
                            if let Err(error) = self
                                .streams_handler
                                .send_streams_through_transport(
                                    transport.clone(),
                                    generator,
                                    port_id,
                                    message_number,
                                )
                                .await
                            {
                                error!("> ServerMessagesHandler > process_server_streams_request > Error while executing StreamsHandler::send_streams_through_transport - Error: {error:?}");
                                if !matches!(error, CommonError::TransportWasClosed) {
                                    // Try to communicate the error to client. Maybe the transport is not closed and it's another type of error
                                    send_remote_error(
                                        transport,
                                        message_number,
                                        ServerError::UnexpectedErrorOnTransport.into(),
                                    )
                                    .await;
                                }
                            }
                        }
                        Err(procedure_remote_error) => {
                            send_remote_error(transport, message_number, procedure_remote_error)
                                .await
                        }
                    },
                    Err(_) => {
                        error!("> ServerMessagesHandler > process_server_streams_request > Error on receiving on a open_ack_listener, sender seems to be dropped");
                    }
                },
                Err(error) => {
                    error!("> ServerMessagesHandler > process_server_streams_request > Erron on opening a server stream: {error:?}");
                    if !matches!(
                        error,
                        ServerResultError::Internal(ServerInternalError::TransportWasClosed)
                    ) {
                        // Try to communicate the error to client. Maybe the transport is not closed and it's another type of error
                        send_remote_error(
                            transport,
                            message_number,
                            ServerError::UnexpectedErrorOnTransport.into(),
                        )
                        .await;
                    }
                }
            }
        });
    }

    /// Receive a client streams procedure handler returned future and process it in a spawned background task.
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    pub fn process_client_streams_request<T: Transport + ?Sized + 'static>(
        self: Arc<Self>,
        transport: Arc<T>,
        message_number: u32,
        client_stream_id: u32,
        procedure_handler: ClientStreamsResponse,
        listener: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        tokio::spawn(async move {
            self.register_listener(client_stream_id, listener).await;
            match procedure_handler.await {
                Ok(procedure_response) => {
                    self.send_response(transport, message_number, procedure_response)
                        .await;
                }
                Err(procedure_remote_err) => {
                    send_remote_error(transport, message_number, procedure_remote_err).await;
                }
            }
        });
    }

    /// Receive a bidirectional streams procedure handler returned future and process it in a spawned background task.
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    pub fn process_bidir_streams_request<T: Transport + ?Sized + 'static>(
        self: Arc<Self>,
        transport: Arc<T>,
        message_number: u32,
        port_id: u32,
        client_stream_id: u32,
        listener: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
        procedure_handler: BiStreamsResponse,
    ) {
        tokio::spawn(async move {
            self.register_listener(client_stream_id, listener).await;
            match self
                .open_server_stream(transport.clone(), message_number, port_id)
                .await
            {
                Ok(open_ack_listener) => {
                    match open_ack_listener.await {
                        Ok(_) => {
                            match procedure_handler.await {
                                Ok(generator) => {
                                    if let Err(err) = self
                                        .streams_handler
                                        .send_streams_through_transport(
                                            transport.clone(),
                                            generator,
                                            port_id,
                                            message_number,
                                        )
                                        .await
                                    {
                                        error!("> ServerMessagesHandler > process_bidir_streams_request > Error while executing StreamsHandler::send_streams_through_transport - Error: {err:?}");
                                        if !matches!(err, CommonError::TransportWasClosed) {
                                            // Try to communicate the error to client. Maybe the transport is not closed and it's another type of error
                                            send_remote_error(
                                                transport,
                                                message_number,
                                                ServerError::UnexpectedErrorOnTransport.into(),
                                            )
                                            .await;
                                        }
                                    }
                                }
                                Err(remote_error) => {
                                    send_remote_error(transport, message_number, remote_error)
                                        .await;
                                }
                            }
                        }
                        Err(_) => {
                            error!("> ServerMessagesHandler > process_bidir_streams_request > Error on receiving on a open_ack_listener, sender seems to be dropped");
                        }
                    }
                }
                Err(err) => {
                    if !matches!(
                        err,
                        ServerResultError::Internal(ServerInternalError::TransportWasClosed)
                    ) {
                        // Try to communicate the error to client. Maybe the transport is not closed and it's another type of error
                        send_remote_error(
                            transport,
                            message_number,
                            ServerError::UnexpectedErrorOnTransport.into(),
                        )
                        .await;
                    }
                }
            }
        });
    }

    /// Notify the listener for a client streams procedure that the client sent a new [`StreamMessage`]
    ///
    /// This function aims to run the procedure handler in spawned task to achieve processing requests concurrently.
    pub fn notify_new_client_stream(self: Arc<Self>, message_number: u32, payload: Vec<u8>) {
        tokio::spawn(async move {
            let lock = self.listeners.lock().await;
            let listener = lock.get(&message_number);
            if let Some(listener) = listener {
                if let Ok(stream_message) = StreamMessage::decode(payload.as_slice()) {
                    if listener
                        .send((
                            RpcMessageTypes::StreamMessage,
                            message_number,
                            stream_message,
                        ))
                        .await
                        .is_err()
                    {
                        error!("> ServerMessagesHandler > notify_new_client_stream > Error while sending through the listener, channel seems to be closed ");
                    }
                } else {
                    error!("> ServerMessagesHandler > notify_new_client_stream > Error while decoding payload into StreamMessage, something is corrupted or bad implemented");
                }
            }
        });
    }

    /// Sends a common response [`Response`] through the given transport
    ///
    /// If it fails to send the response, it will retry it as long as the [`Transport::send`] doesn't return a [`TransportError::Closed`]
    ///
    pub async fn send_response<T: Transport + ?Sized>(
        &self,
        transport: Arc<T>,
        message_number: u32,
        payload: Vec<u8>,
    ) {
        let response = Response {
            message_identifier: build_message_identifier(
                RpcMessageTypes::Response as u32,
                message_number,
            ),
            payload,
        };

        if let Err(err) = transport.send(response.encode_to_vec()).await {
            if !matches!(err, TransportError::Closed) {
                error!("> ServerMessagesHandler > send_response > Error while sending the original response through transport but it seems not to be closed");
                send_remote_error(
                    transport,
                    message_number,
                    ServerError::UnexpectedErrorOnTransport.into(),
                )
                .await;
            } else {
                error!("> ServerMessagesHandler > send_response > Error while sending response through transport, it seems to be clsoed");
            }
        }
    }

    /// Sends a [`StreamMessage`] in order to open the stream on the other half
    async fn open_server_stream<T: Transport + ?Sized>(
        &self,
        transport: Arc<T>,
        message_number: u32,
        port_id: u32,
    ) -> ServerResult<OneShotReceiver<Vec<u8>>> {
        let opening_message = StreamMessage {
            closed: false,
            ack: false,
            sequence_id: 0,
            message_identifier: build_message_identifier(
                RpcMessageTypes::StreamMessage as u32,
                message_number,
            ),
            port_id,
            payload: vec![],
        };

        let receiver = self
            .streams_handler
            .send_stream(transport, opening_message)
            .await
            .map_err(|err| {
                if matches!(err, CommonError::TransportWasClosed) {
                    return ServerResultError::Internal(ServerInternalError::TransportWasClosed);
                }
                ServerResultError::Internal(ServerInternalError::TransportError)
            })?;

        Ok(receiver)
    }

    /// Register a listener for a specific message_id used for client and bidirectional streams
    pub async fn register_listener(
        &self,
        message_id: u32,
        callback: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        let mut lock = self.listeners.lock().await;
        lock.insert(message_id, callback);
    }

    /// Unregister a listener for a specific message_id used for client and bidirectional streams
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
#[cfg(feature = "client")]
pub struct ClientMessagesHandler<T: Transport + ?Sized> {
    /// Transport received by a `RpcClient`
    pub transport: Arc<T>,
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
    process_cancellation_token: tokio_util::sync::CancellationToken,
}

#[cfg(feature = "client")]
impl<T: Transport + ?Sized + 'static> ClientMessagesHandler<T> {
    pub fn new(transport: Arc<T>) -> Self {
        Self {
            transport,
            one_time_listeners: Mutex::new(HashMap::new()),
            process_cancellation_token: tokio_util::sync::CancellationToken::new(),
            listeners: Mutex::new(HashMap::new()),
            streams_handler: Arc::new(StreamsHandler::new()),
        }
    }

    /// Starts a background task to listen responses from the [`RpcServer`](crate::server::RpcServer)  sent to the transport.
    ///
    /// The receiver is an [`Arc<Self>`] in order to be able to process in a backgroun taks and mutate the state of the listeners
    ///
    pub fn start(self: Arc<Self>) {
        use tokio::select;
        let token = self.process_cancellation_token.clone();
        tokio::spawn(async move {
            select! {
                _ = token.cancelled() => {
                    debug!("> ClientMessagesHandler > cancelled!");
                    self.transport.close().await;
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
        use crate::rpc_protocol::parse::parse_header;
        loop {
            match self.transport.receive().await {
                Ok(data) => {
                    let message_header = parse_header(&data);
                    match message_header {
                        Some((message_type, message_number)) => {
                            let mut read_callbacks = self.one_time_listeners.lock().await;
                            // We remove the listener in order to get a owned value and also remove it from memory, it's a one time listener. It's used just this time.
                            let sender = read_callbacks.remove(&message_number);
                            if let Some(sender) = sender {
                                match sender.send(data) {
                                    Ok(()) => {}
                                    Err(_) => {
                                        error!(
                                            "> ClientMessagesHandler > process > error while sending {} response",
                                            message_number
                                        );
                                        continue;
                                    }
                                }
                            } else {
                                // If there is no one time listener for the message_number, we check if there is a recurrent listener (StreamMessages)
                                let listeners = self.listeners.lock().await;
                                let listener = listeners.get(&message_number);

                                if let Some(listener) = listener {
                                    if let Ok(stream_message) =
                                        StreamMessage::decode(data.as_slice())
                                    {
                                        if let Err(error) = listener
                                            .send((message_type, message_number, stream_message))
                                            .await
                                        {
                                            error!(
                                                "> ClientMessagesHandler > process > Error while sending StreamMessage to a listener {error:?}")
                                        }
                                    } else {
                                        error!("> ClientMessagesHandler > process > Error while decoding bytes into a StreamMessage, something seems to be bad implemented")
                                    }
                                } else {
                                    // If there is no listener for the message, then it's an ACK message for a StreamMessage from the Server
                                    self.streams_handler
                                        .clone()
                                        .message_acknowledged_by_peer(message_number, data)
                                }
                            }
                        }
                        None => {
                            // TODO: If the bytes cannot be parsed to get the header, we should implement something to clean receiver/sender from the client state
                            error!("> ClientMessagesHandler > process > Error on parsing message header - impossible to communicate the error to a listener, the message is corrupted or invalid");
                            continue;
                        }
                    }
                }
                Err(error) => {
                    error!("> ClientMessagesHandler > process > Error on receive {error:?}");
                    if matches!(error, TransportError::Closed) {
                        log::info!("> ClientMessagesHandler > process > closing...");
                        break;
                    }
                }
            }
        }
    }

    /// It spawns a background task to wait for the server to acknowledge the open of client streams or biderectional streams.
    ///
    ///  After the server acknowledges the open, it starts sending stram messages.
    ///
    /// The receiver of the function is an [`Arc<Self>`] because an instance should be cloned for the background task and mutate the state of the message listeners
    ///
    pub fn await_server_ack_open_and_send_streams<M: Message + 'static>(
        self: Arc<Self>,
        open_promise: OneShotReceiver<Vec<u8>>,
        client_stream: Generator<M>,
        port_id: u32,
        client_message_id: u32,
    ) {
        let transport = self.transport.clone();
        tokio::spawn(async move {
            match open_promise.await {
                Ok(encoded_ack_response) => {
                    if let Ok(stream_message) =
                        StreamMessage::decode(encoded_ack_response.as_slice())
                    {
                        if stream_message.closed {
                            return;
                        }

                        let new_generator = Generator::from_generator(client_stream, |item| {
                            Some(item.encode_to_vec())
                        });

                        if let Err(error) = self
                            .streams_handler
                            .send_streams_through_transport(
                                transport,
                                new_generator,
                                port_id,
                                client_message_id,
                            )
                            .await
                        {
                            error!("> ClientMessagesHandler > await_server_ack_open_and_send_streams > Error while executing StreamsHandler::send_streams_through_transport - Error: {error:?}");
                        }
                    } else {
                        error!("> ClientMessagesHandler > await_server_ack_open_and_send_streams > Error while decoding bytes into StreamMessage")
                    }
                }
                Err(_) => {
                    error!("> ClientMessagesHandler > await_server_ack_open_and_send_streams > Error while awaiting the server to send the ACK for Open stream message, sender half seems to be dropped");
                }
            }
        });
    }

    /// Registers a one time listener. It will be used only one time and then removed.
    pub async fn register_one_time_listener(
        &self,
        message_number: u32,
        callback: OneShotSender<Vec<u8>>,
    ) {
        let mut lock = self.one_time_listeners.lock().await;
        lock.insert(message_number, callback);
    }

    /// Registers a listener which will be more than one time
    pub async fn register_listener(
        &self,
        message_number: u32,
        callback: AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        let mut lock = self.listeners.lock().await;
        lock.insert(message_number, callback);
    }

    /// Unregister a listener
    pub async fn unregister_listener(&self, message_number: u32) {
        let mut lock = self.listeners.lock().await;
        lock.remove(&message_number);
    }
}

/// In charge of handling the acknowledge listeners for Stream Messages so that it knows that it has to send the next [`StreamMessage`]
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

    /// It sends a message through the given `transport` in the parameter to close an opened stream procedure
    async fn close_stream<T: Transport + ?Sized>(
        &self,
        transport: Arc<T>,
        sequence_id: u32,
        message_number: u32,
        port_id: u32,
    ) -> Result<(), CommonError> {
        let close_message = StreamMessage {
            closed: true,
            ack: false,
            sequence_id,
            message_identifier: build_message_identifier(
                RpcMessageTypes::StreamMessage as u32,
                message_number,
            ),
            port_id,
            payload: vec![],
        };

        transport
            .send(close_message.encode_to_vec())
            .await
            .map_err(|_| CommonError::TransportError)?;

        Ok(())
    }

    /// As it receives encoded messages from the `stream_generator`, it'll be sending [`StreamMessage`]s through the given transport in the parameters.
    ///
    /// It handles the sequence id for each [`StreamMessage`], it'll await for the acknowlegde of each message in the other half to conitnue with the messages sending.
    ///
    /// Also, it stops the generator and break the loop if the other half closed the stream. Otherwise, it will close the strram when the `stream_generator` doesn't have more messages.
    ///
    pub async fn send_streams_through_transport<T: Transport + ?Sized>(
        &self,
        transport: Arc<T>,
        mut stream_generator: Generator<Vec<u8>>,
        port_id: u32,
        message_number: u32,
    ) -> Result<(), CommonError> {
        let mut sequence_number = 0;
        let mut was_closed_by_peer = false;
        while let Some(message) = stream_generator.next().await {
            sequence_number += 1;
            let current_message = StreamMessage {
                closed: false,
                ack: false,
                sequence_id: sequence_number,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::StreamMessage as u32,
                    message_number,
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
                            Err(_) => {
                                error!("> StreamsHandler > send_streams_through_transport > Error while decoding bytes into a StreamMessage");
                                return Err(CommonError::ProtocolError);
                            }
                        },
                        Err(_) => {
                            error!("> StreamsHandler > send_streams_through_transport > Error while waiting for an ACK Message, the sender half seems to be dropped.");
                            return Err(CommonError::UnexpectedError(
                                "The sender half of a listener seems to be droppped".to_string(),
                            ));
                        }
                    };
                    if ack_message.ack {
                        debug!("> StreamsHandler > send_streams_through_transport > Listener received the ack for a message, continuing with the next stream");
                        continue;
                    } else if ack_message.closed {
                        debug!("> StreamsHandler > send_streams_through_transport > stream was closed by the other peer");
                        was_closed_by_peer = true;
                        stream_generator.close();
                        break;
                    }
                }
                Err(err) => {
                    error!("> StreamsHandler > send_streams_through_transport > Error while streaming a server stream {err:?}");
                    return Err(err);
                }
            }
        }

        if !was_closed_by_peer {
            self.close_stream(transport, sequence_number, message_number, port_id)
                .await?;
        }

        Ok(())
    }

    /// Sends a [`StreamMessage`] through the given transport and registers the created acknowledge listener for the sent message and return it.
    ///
    /// If it fails to send the[`StreamMessage`], the function will try to send a [`RemoteError`] to notify the client as long as [`Transport::send`] doesn't return [`TransportError::Closed`]
    ///
    async fn send_stream<T: Transport + ?Sized>(
        &self,
        transport: Arc<T>,
        message: StreamMessage,
    ) -> Result<OneShotReceiver<Vec<u8>>, CommonError> {
        let (_, message_number) = parse_message_identifier(message.message_identifier);
        let (tx, rx) = oneshot_channel();
        {
            let mut lock = self.ack_listeners.lock().await;
            lock.insert(format!("{}{}", message_number, message.sequence_id), tx);
        }

        if let Err(error) = transport.send(message.encode_to_vec()).await {
            error!("> StreamsHandler > send_stream > Error while sending through transport - message: {message:?} - Error: {error:?}");
            {
                // Remove the inserted tx in order to drop the channel because it won't be used
                let mut lock = self.ack_listeners.lock().await;
                lock.remove(&format!("{}{}", message_number, message.sequence_id));
            }
            if !matches!(error, TransportError::Closed) {
                return Err(CommonError::TransportError);
            } else {
                return Err(CommonError::TransportWasClosed);
            }
        }

        Ok(rx)
    }

    /// Notify the acknowledge listener registered in [`send_stream`](#method.send_stream) that the message was acknowledge by the other peer and it can continue sending the pending messages
    pub fn message_acknowledged_by_peer(self: Arc<Self>, message_number: u32, payload: Vec<u8>) {
        tokio::spawn(async move {
            match parse_protocol_message::<StreamMessage>(&payload) {
                Ok((_, _, stream_message)) => {
                    let listener = {
                        let mut lock = self.ack_listeners.lock().await;
                        // we should remove ack listener it just for a seq_id
                        lock.remove(&format!("{}{}", message_number, stream_message.sequence_id))
                    };
                    match listener {
                        Some(sender) => {
                            if sender.send(payload).is_err() {
                                error!("> Streams Handler > message_acknowledged_by_peer > Error while sending through the ack listener, seems to be dropped")
                            }
                        }
                        None => {
                            debug!("> Streams Handler > message_acknowledged_by_peer > ack listener not found")
                        }
                    }
                }
                Err(ParseErrors::IsARemoteError((_, remote_error))) => {
                    error!("> Streams Handler > message_acknowledged_by_peer > Remote Error: {remote_error:?}")
                }
                Err(err) => {
                    error!("> Streams Handler > message_acknowledged_by_peer > Error on parsing: {err:?}");
                }
            }
        });
    }
}

/// Reusable function for sending a remote erorr
async fn send_remote_error<T: Transport + ?Sized>(
    transport: Arc<T>,
    message_number: u32,
    mut remote_error: RemoteError,
) {
    // We have to complete the RemoteError message becaue the message_identifier is 0 because
    // `message_number` (message_identifier) is not given to the procedure_handler and it's unable to build the identifier
    fill_remote_error(&mut remote_error, message_number);
    if let Err(err) = transport.send(remote_error.encode_to_vec()).await {
        error!("> send_remote_error > Error while sending the remote error through a transport > RemoteError: {remote_error:?} - Error: {err:?}")
    }
}
