use std::{future::Future, sync::Arc};

use log::debug;
use prost::Message;

use tokio::{
    select,
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
};

use async_channel::{unbounded, Receiver as AsyncChannelReceiver, Sender as AsyncChannelSender};

use tokio_util::sync::CancellationToken;

use crate::{
    rpc_protocol::{parse::build_message_identifier, RpcMessageTypes, StreamMessage},
    transports::{Transport, TransportError},
    CommonError,
};

/// It knows how to process all the stream messages for client streams, server streams and bidirectional streams.
///
/// And it should be used to consume the stream messages when a stream procedure is executed
///
pub struct StreamProtocol<T: Transport + ?Sized> {
    /// the ID of Port
    port_id: u32,
    /// ID of the message that inited the streaming
    message_number: u32,
    /// The last sequence id received in a [`StreamMessage`]
    last_received_sequence_id: u32,
    /// Flag to know if the remote half is closed or closed the stream
    is_remote_closed: bool,
    /// Flag to know if the remote half was open in the last message
    was_open: bool,
    /// Generator field in charge of both half of a generator
    ///
    /// [`Generator`] in charge of waiting for the next stream message
    ///
    /// [`GeneratorYielder`] in charge of sending the next stream message to the [`Generator`]
    ///
    generator: (Generator<Vec<u8>>, GeneratorYielder<Vec<u8>>),
    /// The transport used for the communications
    transport: Arc<T>,
    /// Cancellation token to cancel the background task listening for new messages
    process_cancellation_token: CancellationToken,
}

impl<T: Transport + ?Sized + 'static> StreamProtocol<T> {
    pub(crate) fn new(transport: Arc<T>, port_id: u32, message_number: u32) -> Self {
        Self {
            last_received_sequence_id: 0,
            is_remote_closed: false,
            was_open: false,
            generator: Generator::create(),
            transport,
            message_number,
            port_id,
            process_cancellation_token: CancellationToken::new(),
        }
    }

    /// Get the next received stream message
    ///
    /// It'll be sent by the [`GeneratorYielder`] but actually the message comes from the other half ([`crate::server::RpcServer`] or [`crate::client::RpcClient`] )
    ///
    /// You won't use this function direcly, you should turn the [`StreamProtocol`] into a [`Generator`] using [`to_generator`](#method.to_generator)
    ///
    async fn next(&mut self) -> Option<Vec<u8>> {
        select! {
            _ = self.process_cancellation_token.cancelled() =>  {
                self.generator.0.close();
                self.is_remote_closed = true;
                None
            }
            message = self.generator.0.next() => {
                match message {
                    Some(msg) => {
                        self.last_received_sequence_id += 1;
                        self.was_open = true;
                        // Ack Message to let know the peer that we are ready to receive another message
                        let stream_message = StreamMessage {
                            port_id: self.port_id,
                            sequence_id: self.last_received_sequence_id,
                            message_identifier: build_message_identifier(
                                RpcMessageTypes::StreamAck as u32,
                                self.message_number,
                            ),
                            payload: vec![],
                            closed: false,
                            ack: true,
                        };

                        self.transport
                            .send(stream_message.encode_to_vec())
                            .await
                            .unwrap();


                        Some(msg)
                    }
                    None => {
                        self.is_remote_closed = true;
                        None
                    }
                }
            }
        }
    }

    /// It consumes the [`StreamProtocol`] and returns a [`Generator`].
    ///
    /// It allows to pass a closure to transform the values that the internal [`Generator`] has.
    ///
    /// It transforms the values and sends it to the returned [`Generator`] in a backgroun taks for a immediate response.
    ///
    pub fn to_generator<O: Send + Sync + 'static, F: Fn(Vec<u8>) -> O + Send + Sync + 'static>(
        mut self,
        transformer: F,
    ) -> Generator<O> {
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            while let Some(item) = self.next().await {
                let new_item = transformer(item);
                generator_yielder.r#yield(new_item).await.unwrap();
            }
        });

        generator
    }

    /// It sends an ACK stream message through the transport given to [`StreamProtocol`] to let the other half know that the strem is opened
    async fn acknowledge_open(&self) -> Result<(), TransportError> {
        let stream_message = StreamMessage {
            port_id: self.port_id,
            sequence_id: self.last_received_sequence_id,
            message_identifier: build_message_identifier(
                RpcMessageTypes::StreamAck as u32,
                self.message_number,
            ),
            payload: vec![],
            closed: false,
            ack: true,
        };

        self.transport.send(stream_message.encode_to_vec()).await
    }

    /// Finishs the stream processing, closes all generators and cancels all background tasks
    pub fn close(&mut self) {
        self.generator.0.close();
        self.process_cancellation_token.cancel();
    }

    /// Spawns a background task to start processing the stream messages and returns a listener ([`AsyncChannelSender`])
    ///
    /// The returned listener will be registered in [`crate::messages_handlers::ServerMessagesHandler`] or [`crate::messages_handlers::ClientMessagesHandler`] that the [`crate::server::RpcServer`] and [`crate::client::RpcClient`] contains
    ///
    /// Each new message that either of both structs receives will be handled by the messages handler and sent to the returned listener if it corresponds
    ///
    /// The callback expected in params, it should be to remove the listener from the messages handler when the the processing finishes
    ///
    pub(crate) async fn start_processing<
        F: Future + Send,
        Callback: FnOnce() -> F + Send + 'static,
    >(
        &self,
        callback: Callback,
    ) -> Result<AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>, CommonError> {
        if self.acknowledge_open().await.is_err() {
            return Err(CommonError::TransportError);
        }
        let token = self.process_cancellation_token.clone();
        let (messages_listener, messages_processor) = unbounded();
        let internal_channel = self.generator.1.clone();
        tokio::spawn(async move {
            let token_cloned = token.clone();
            select! {
                _ = token.cancelled() => {
                    debug!("> StreamProtocol cancelled!");
                    callback().await;
                    // TOOD: Communicate error
                },
                _ = Self::process_messages(messages_processor, internal_channel, token_cloned) => {
                    callback().await;
                }
            }
        });

        Ok(messages_listener)
    }

    /// Processes stream messages, it's called by the background task spawned in [`start_processing`](#method.start_processing)
    ///
    /// It receives each [`StreamMessage`] and decides what to do with the stream messages processing, if it has to continue yielding messages or close the stream processing
    ///
    async fn process_messages(
        messages_processor: AsyncChannelReceiver<(RpcMessageTypes, u32, StreamMessage)>,
        internal_channel_sender: GeneratorYielder<Vec<u8>>,
        cancellation_token: CancellationToken,
    ) {
        while let Ok(message) = messages_processor.recv().await {
            if matches!(message.0, RpcMessageTypes::StreamMessage) {
                if message.2.closed {
                    cancellation_token.cancel();
                    messages_processor.close();
                } else {
                    internal_channel_sender
                        .r#yield(message.2.payload)
                        .await
                        .unwrap();
                }
            } else if matches!(message.0, RpcMessageTypes::RemoteErrorResponse) {
                cancellation_token.cancel();
                messages_processor.close();
                // TOOD: Communicate error
            }
        }
    }
}

/// Errors related with [`Generator`] and [`GeneratorYielder`]
#[derive(Debug)]
pub enum GeneratorError {
    UnableToInsert,
}

/// Generator struct contains only one field which it's an unbounded receiver from unounded channel from [`tokio`] crate
///
/// The other half of the unbounded channel is given to the [`GeneratorYielder`]
///
pub struct Generator<M>(UnboundedReceiver<M>);

impl<M: Send + Sync + 'static> Generator<M> {
    /// Creates an unbounded channel and returns a [`Generator`] and [`GeneratorYielder`]
    pub fn create() -> (Self, GeneratorYielder<M>) {
        let channel = unbounded_channel();
        (Self(channel.1), GeneratorYielder::new(channel.0))
    }

    // TODO: could it be a trait and reuse for other structs? or replace it with From trait
    pub fn from_generator<O: Send + Sync + 'static, F: Fn(M) -> O + Send + Sync + 'static>(
        mut old_generator: Generator<M>,
        transformer: F,
    ) -> Generator<O> {
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            while let Some(item) = old_generator.next().await {
                let new_item = transformer(item);
                generator_yielder.r#yield(new_item).await.unwrap();
            }
        });

        generator
    }

    pub async fn next(&mut self) -> Option<M> {
        self.0.recv().await
    }

    pub fn close(&mut self) {
        self.0.close()
    }
}

/// The other half for a [`Generator`]. It contains an only one field which it's an unbounded sender from a unbounded channel from [`tokio`] crate
///
/// It's in charge of sendin the values to the [`Generator`]
///
pub struct GeneratorYielder<M>(UnboundedSender<M>);

impl<M> GeneratorYielder<M> {
    fn new(sender: UnboundedSender<M>) -> Self {
        Self(sender)
    }

    pub async fn r#yield(&self, item: M) -> Result<(), GeneratorError> {
        match self.0.send(item) {
            Ok(_) => Ok(()),
            Err(_) => Err(GeneratorError::UnableToInsert),
        }
    }
}

impl<M> Clone for GeneratorYielder<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
