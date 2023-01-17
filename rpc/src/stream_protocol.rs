use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use log::debug;
use prost::Message;

use async_channel::{
    unbounded as async_unbounded, Receiver as AsyncChannelReceiver, Sender as AsyncChannelSender,
};
use tokio::select;
use tokio_util::sync::CancellationToken;

use crate::{
    protocol::{parse::build_message_identifier, RpcMessageTypes, StreamMessage},
    transports::{Transport, TransportError},
};

pub struct Stream<M: Message + Default>(pub StreamProtocol<M>);

impl<M: Message + Default> Deref for Stream<M> {
    type Target = StreamProtocol<M>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: Message + Default> DerefMut for Stream<M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct StreamProtocol<M: Message + Default> {
    port_id: u32,
    message_number: u32,
    last_received_sequence_id: u32,
    is_remote_closed: bool,
    was_open: bool,
    channel: (AsyncChannelSender<M>, AsyncChannelReceiver<M>),
    transport: Arc<dyn Transport + Send + Sync>,
    messages_processor: AsyncChannelReceiver<(RpcMessageTypes, u32, StreamMessage)>,
    process_cancellation_token: CancellationToken,
}

impl<M: Message + Default + 'static> StreamProtocol<M> {
    pub(crate) fn create(
        transport: Arc<dyn Transport + Send + Sync>,
        port_id: u32,
        message_number: u32,
    ) -> (
        Self,
        AsyncChannelSender<(RpcMessageTypes, u32, StreamMessage)>,
    ) {
        let (tx, rx) = async_unbounded();

        (
            Self {
                last_received_sequence_id: 0,
                is_remote_closed: false,
                was_open: false,
                channel: async_unbounded(),
                transport,
                message_number,
                port_id,
                messages_processor: rx,
                process_cancellation_token: CancellationToken::new(),
            },
            tx,
        )
    }

    pub async fn next(&mut self) -> Option<M> {
        match self.channel.1.recv().await {
            Ok(msg) => {
                self.last_received_sequence_id += 1;
                self.was_open = true;
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
            Err(_) => {
                self.is_remote_closed = true;
                None
            }
        }
    }

    pub(crate) async fn acknowledge_open(&self) -> Result<(), TransportError> {
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

    pub fn close(&self) {
        self.channel.1.close();
        self.process_cancellation_token.cancel();
    }

    pub fn start_processing(&self) {
        let token = self.process_cancellation_token.clone();
        let messages_processor = self.messages_processor.clone();
        let internal_channel = (self.channel.0.clone(), self.channel.1.clone());
        tokio::spawn(async move {
            select! {
                _ = token.cancelled() => {
                    debug!("> StreamProtocol cancelled!")
                },
                _ = Self::process_messages(messages_processor, internal_channel) => {

                }
            }
        });
    }

    async fn process_messages(
        messages_processor: AsyncChannelReceiver<(RpcMessageTypes, u32, StreamMessage)>,
        internal_channel: (AsyncChannelSender<M>, AsyncChannelReceiver<M>),
    ) {
        while let Ok(message) = messages_processor.recv().await {
            if matches!(message.0, RpcMessageTypes::StreamMessage) {
                if message.2.closed {
                    internal_channel.1.close();
                } else {
                    let decoded = M::decode(message.2.payload.as_slice()).unwrap();
                    internal_channel.0.send(decoded).await.unwrap();
                }
            }
        }
    }
}
