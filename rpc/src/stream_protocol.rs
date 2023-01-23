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
    protocol::{parse::build_message_identifier, RpcMessageTypes, StreamMessage},
    transports::{Transport, TransportError},
    CommonError,
};

pub struct StreamProtocol {
    port_id: u32,
    message_number: u32,
    last_received_sequence_id: u32,
    is_remote_closed: bool,
    was_open: bool,
    pub generator: (Generator<Vec<u8>>, GeneratorYielder<Vec<u8>>),
    transport: Arc<dyn Transport + Send + Sync>,
    process_cancellation_token: CancellationToken,
}

impl StreamProtocol {
    pub(crate) fn new(
        transport: Arc<dyn Transport + Send + Sync>,
        port_id: u32,
        message_number: u32,
    ) -> Self {
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

    pub fn to_generator<O: Send + Sync + 'static, F: Fn(Vec<u8>) -> O + Send + Sync + 'static>(
        mut self,
        transformer: F,
    ) -> Generator<O> {
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            while let Some(item) = self.next().await {
                let new_item = transformer(item);
                generator_yielder.insert(new_item).await.unwrap();
            }
        });

        generator
    }

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

    pub fn close(&mut self) {
        self.generator.0.close();
        self.process_cancellation_token.cancel();
    }

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
                        .insert(message.2.payload)
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

#[derive(Debug)]
pub enum GeneratorError {
    UnableToInsert,
}

pub struct Generator<M>(UnboundedReceiver<M>);

impl<M: Send + Sync + 'static> Generator<M> {
    pub fn create() -> (Self, GeneratorYielder<M>) {
        let channel = unbounded_channel();
        (Self(channel.1), GeneratorYielder::new(channel.0))
    }

    // TODO: could it be a trait and reuse for other structs?
    pub fn from_generator<O: Send + Sync + 'static, F: Fn(M) -> O + Send + Sync + 'static>(
        mut old_generator: Generator<M>,
        transformer: F,
    ) -> Generator<O> {
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            while let Some(item) = old_generator.next().await {
                let new_item = transformer(item);
                generator_yielder.insert(new_item).await.unwrap();
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

pub struct GeneratorYielder<M>(UnboundedSender<M>);

impl<M> GeneratorYielder<M> {
    fn new(sender: UnboundedSender<M>) -> Self {
        Self(sender)
    }

    pub async fn insert(&self, item: M) -> Result<(), GeneratorError> {
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
