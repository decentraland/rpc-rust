//! This module contains all the types needed to have a running [`RpcServer`].
//
use std::{collections::HashMap, sync::Arc, u8};

use log::{debug, error};
use prost::{alloc::vec::Vec, Message};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::{
    messages_handlers::ServerMessagesHandler,
    rpc_protocol::{
        fill_remote_error,
        parse::{build_message_identifier, parse_header},
        server_ready_message, CreatePort, CreatePortResponse, DestroyPort, ModuleProcedure,
        RemoteError, RemoteErrorResponse, Request, RequestModule, RequestModuleResponse,
        RpcMessageTypes,
    },
    service_module_definition::{ProcedureDefinition, ServiceModuleDefinition},
    stream_protocol::StreamProtocol,
    transports::{Transport, TransportError, TransportMessage},
};

/// Handler that runs each time that a port is created
type PortHandlerFn<Context> = dyn Fn(&mut RpcServerPort<Context>) + Send + Sync + 'static;

type OnTransportClosesHandler<Transport> = dyn Fn(Arc<Transport>) + Send + Sync + 'static;

/// Error returned by a server function could be an error which it's possible and useful to communicate or not.
#[derive(Debug)]
pub enum ServerResultError {
    External(ServerError),
    Internal(ServerInternalError),
}

/// Result type for all [`RpcServer`] functions
pub type ServerResult<T> = Result<T, ServerResultError>;

/// Enum of errors which should be exposed to the client and turned into a [`crate::rpc_protocol::RemoteError`]
#[derive(Debug)]
pub enum ServerError {
    /// Error on decoding bytes (`Vec<u8>`) into a given type using [`crate::rpc_protocol::parse::parse_protocol_message`] or using the [`Message::decode`]
    ProtocolError,
    /// Port was not found in the server state, possibly not created
    PortNotFound,
    /// Error on loading a Module, unlikely to happen
    LoadModuleError,
    /// Module was not found, not registered in the server
    ModuleNotFound,
    /// Given procedure's ID was not found
    ProcedureNotFound,
    /// Unexpexted Error while responding back or Error on sending the original procedure response
    ///
    /// This error should be use as a "re-try" when a [`Transport::send`] failed.
    UnexpectedErrorOnTransport,
}

impl RemoteErrorResponse for ServerError {
    fn error_code(&self) -> u32 {
        match self {
            Self::ProtocolError => 1,
            Self::PortNotFound => 2,
            Self::ModuleNotFound => 3,
            Self::ProcedureNotFound => 4,
            Self::UnexpectedErrorOnTransport => 5,
            Self::LoadModuleError => 0, // it's unlikely to happen
        }
    }

    fn error_message(&self) -> String {
        match self {
            Self::ProtocolError => "Error on parsing a message. The content seems to be corrupted and not to meet the protocol requirements".to_string(),
            Self::PortNotFound => "The given Port's ID was not found".to_string(),
            Self::LoadModuleError => "Error on loading a module".to_string(),
            Self::ModuleNotFound => "Module wasn't found on the server, check the name".to_string(),
            Self::ProcedureNotFound => "Procedure's ID wasn't found on the server".to_string(),
            Self::UnexpectedErrorOnTransport => "Error on the transport while sending the original procedure response".to_string()
        }
    }
}

/// Enum of errors which are internal or have no sense to be exposed to the client
#[derive(Debug)]
pub enum ServerInternalError {
    UnableToNofifyServer,
    TransportError,
    TransportNotAttached,
    InvalidHeader,
    TransportWasClosed,
}

type TransportID = u32;
type PortID = u32;

type TransportEvent<T, M> = (T, M);

/// Events that the [`RpcServer`] has to react to
enum ServerEvents<T: Transport + ?Sized> {
    AttachTransport(Arc<T>),
    NewTransport(TransportID, Arc<T>),
}

/// Notifications about Transports connected to the [`RpcServer`]
enum TransportNotification<T: Transport + ?Sized> {
    /// New message received from a transport
    NewMessage(TransportEvent<(Arc<T>, TransportID), TransportMessage>),
    /// A Notification for when a `ServerEvents::AttachTransport` is received in order to attach a transport to the server [`RpcServer`](#method.RpcServer.attach_transport) and make it run to receive messages
    MustAttachTransport(Arc<T>),
    /// Close Transport Notification in order to remove it from the [`RpcServer`] state
    CloseTransport(TransportID),
}

/// Structure to send events to the server from outside. It's a wrapper for a [`tokio::sync::mpsc::UnboundedSender`] from a channel so that we can send events from another thread e.g for a Websocket listener.
pub struct ServerEventsSender<T: Transport + ?Sized>(UnboundedSender<ServerEvents<T>>);

impl<T: Transport + ?Sized> ServerEventsSender<T> {
    /// Sends a [`ServerEvents::AttachTransport`] to the [`RpcServer`]
    ///
    /// This allows you to notify the server that has to attach a new transport so after that it can make it run to listen for new messages
    ///
    /// This is equivalent to `RpcServer::attach_transport` but it can be used to attach a transport to the [`RpcServer`] from another spawned thread (or background task)
    ///
    /// This allows you to listen on a port in a background task for external connections and attach multiple transports that want to connect to the server
    ///
    /// It receives the `Transport` inside an `Arc` because it must be sharable.
    ///
    pub fn send_attach_transport(&self, transport: Arc<T>) -> ServerResult<()> {
        if self
            .0
            .send(ServerEvents::AttachTransport(transport))
            .is_err()
        {
            return Err(ServerResultError::Internal(
                ServerInternalError::UnableToNofifyServer,
            ));
        }
        Ok(())
    }

    /// Sends a [`ServerEvents::NewTransport`] to the [`RpcServer`]
    ///
    /// This allows you to notify the server that has to put to run a new transport
    ///
    /// It receives the [`Transport`] inside an `Arc` because it must be sharable.
    ///
    fn send_new_transport(&self, id: TransportID, transport: Arc<T>) -> ServerResult<()> {
        if self
            .0
            .send(ServerEvents::NewTransport(id, transport))
            .is_err()
        {
            error!("> RpcServer > Error on notifying the new transport {id}");
            return Err(ServerResultError::Internal(
                ServerInternalError::TransportNotAttached,
            ));
        }
        Ok(())
    }
}

impl<T: Transport + ?Sized> Clone for ServerEventsSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// RpcServer receives and process different requests from the RpcClient
///
/// Once a RpcServer is inited, you should attach a transport and handler
/// for the port creation.
pub struct RpcServer<Context, T: Transport + ?Sized> {
    /// The Transport used for the communication between `RpcClient` and [`RpcServer`]
    transports: HashMap<TransportID, Arc<T>>,
    /// The handler executed when a new port is created
    port_creation_handler: Option<Box<PortHandlerFn<Context>>>,
    /// The handler executed when a transport is closed.
    ///
    /// It works for cleaning resources that may be tied to or depends on the transport's connection.
    ///
    on_transport_closes_handler: Option<Box<OnTransportClosesHandler<T>>>,
    /// Ports registered in the [`RpcServer`]
    ports: HashMap<PortID, RpcServerPort<Context>>,
    ports_by_transport_id: HashMap<TransportID, Vec<PortID>>,
    /// RpcServer Context
    context: Arc<Context>,
    /// Handler in charge of handling every request<>response.
    ///
    /// It's stored inside an `Arc` because it'll be shared between threads
    messages_handler: Arc<ServerMessagesHandler>,
    /// `ServerEventsSender` structure that contains the sender half of a channel to send `ServerEvents` to the [`RpcServer`]
    server_events_sender: ServerEventsSender<T>,
    /// The receiver half of a channel that receives `ServerEvents` which the [`RpcServer`] has to react to
    ///
    /// It's an Option so that we can take ownership of it and remove it from the [`RpcServer`], and make it run in a background task
    server_events_receiver: Option<UnboundedReceiver<ServerEvents<T>>>,
    /// The id that will be assigned if a new transport is a attached
    next_transport_id: u32,
}
impl<Context: Send + Sync + 'static, T: Transport + ?Sized + 'static> RpcServer<Context, T> {
    pub fn create(ctx: Context) -> Self {
        let channel = unbounded_channel();
        Self {
            transports: HashMap::new(),
            port_creation_handler: None,
            on_transport_closes_handler: None,
            ports: HashMap::new(),
            ports_by_transport_id: HashMap::new(),
            context: Arc::new(ctx),
            messages_handler: Arc::new(ServerMessagesHandler::new()),
            next_transport_id: 1,
            server_events_sender: ServerEventsSender(channel.0),
            server_events_receiver: Some(channel.1),
        }
    }

    /// Get a `ServerEventsSender` to send allowed server events from outside
    pub fn get_server_events_sender(&self) -> ServerEventsSender<T> {
        self.server_events_sender.clone()
    }

    /// Attaches the server half of the transport for Client<>Server communications
    ///
    /// It differs from sending the `ServerEvents::AtacchTransport` because it can only be used to attach transport from the current thread where the [`RpcServer`] was initalized due to the mutably borrow
    ///
    /// It receives the `Transport` inside an `Arc` because it must be sharable.
    ///
    pub async fn attach_transport(&mut self, transport: Arc<T>) -> ServerResult<()> {
        self.new_transport_attached(transport).await
    }

    /// Sends the `ServerEvents::NewTransport` in order to make this new transport run in backround to receive its messages
    ///
    /// This function is used when a transport is attached with`RpcServer::attach_transport` and with the `ServerEventsSender::send_attach_transport`
    ///
    /// It receives the `Transport` inside an `Arc` because it must be sharable.
    ///
    async fn new_transport_attached(&mut self, transport: Arc<T>) -> ServerResult<()> {
        let current_id = self.next_transport_id;
        if let Err(error) = transport.send(server_ready_message().encode_to_vec()).await {
            error!("> RpcServer > new_transport_attached > Error while sending server ready message: {error:?}");
            if matches!(error, TransportError::Closed) {
                return Err(ServerResultError::Internal(
                    ServerInternalError::TransportError,
                ));
            } else {
                transport.close().await;
                return Err(ServerResultError::Internal(
                    ServerInternalError::TransportError,
                ));
            }
        }
        self.server_events_sender
            .send_new_transport(current_id, transport.clone())?;
        self.transports.insert(current_id, transport);
        self.next_transport_id += 1;
        Ok(())
    }

    /// Start processing `ServerEvent` events and listening on a channel for new `TransportNotification` that are sent by all the attached transports that are running in background tasks.
    pub async fn run(&mut self) {
        // create transports notifier. This channel will be in charge of sending all messages (and errors) that all the transports attached to server receieve
        // We use async_channel crate for this channel because we want our receiver to be cloned so that we can close it when no more transports are open
        // And after that, our server can exit because it knows that it wont receive more notifications
        let (transports_notifier, mut transports_notification_receiver) =
            unbounded_channel::<TransportNotification<T>>();
        // Spawn a task to process ServerEvents in background
        self.process_server_events(transports_notifier);
        // loop on transports_notifier
        loop {
            // A transport here is the equivalent to a new connection in a common HTTP server
            match transports_notification_receiver.recv().await {
                Some(notification) => match notification {
                    TransportNotification::NewMessage(((transport, transport_id), event)) => {
                        match parse_header(&event) {
                            Some((message_type, message_number)) => {
                                match self
                                    .handle_message(
                                        transport_id,
                                        event,
                                        message_type,
                                        message_number,
                                    )
                                    .await
                                {
                                    Ok(_) => debug!("> RpcServer > Transport message handled!"),
                                    Err(server_error) => match server_error {
                                        ServerResultError::External(server_external_error) => {
                                            error!("> RpcServer > Server External Error {server_external_error:?}");
                                            // If a server error is external, we should send it back to the client
                                            tokio::spawn(async move {
                                                let mut remote_error: RemoteError =
                                                    server_external_error.into();
                                                fill_remote_error(
                                                    &mut remote_error,
                                                    message_number,
                                                );
                                                if transport
                                                    .send(remote_error.encode_to_vec())
                                                    .await
                                                    .is_err()
                                                {
                                                    error!("> RpcServer > Error on sending the a RemoteError to the client {remote_error:?}")
                                                }
                                            });
                                        }
                                        ServerResultError::Internal(server_internal_error) => {
                                            error!("> RpcServer > Server Internal Error: {server_internal_error:?}")
                                        }
                                    },
                                }
                            }
                            None => {
                                error!("> RpcServer > A Invalid Header was sent by the client, message ignored");
                                continue;
                            }
                        }
                    }
                    TransportNotification::MustAttachTransport(transport) => {
                        if let Err(error) = self.new_transport_attached(transport).await {
                            error!("> RpcServer > Error on attaching transport to the server in order to receive message from it: {error:?}");
                            continue;
                        }
                    }
                    TransportNotification::CloseTransport(id) => {
                        if let Some(transport) = self.transports.remove(&id) {
                            if let Some(on_close_handler) = &self.on_transport_closes_handler {
                                on_close_handler(transport);
                            }
                            // Get port ids to drop ports
                            if let Some(port_ids) = self.ports_by_transport_id.remove(&id) {
                                for id in port_ids {
                                    // Drop port
                                    self.ports.remove(&id);
                                }
                            }
                        }
                    }
                },
                None => {
                    error!("> RpcServer > Transport notification receiver error");
                    break;
                }
            }
        }
    }

    /// Process `ServerEvent` that are sent through the events channel.
    ///
    /// It spawns a background task to listen on the channel for new events and executes different actions depending on the event.
    ///
    /// # Events
    /// - `ServerEvent::NewTransport` : Spawns a background task to listen on the transport for new `TransportEvent` and then it sends that new event to the [`RpcServer`]
    /// - `ServerEvent::TransportFinished` : Collect in memory the amount of transports that already finished and when the amount is equal to the total running transport, it emits `ServerEvents::Terminated`
    /// - `ServerEvent::Terminated` : Close the [`RpcServer`] transports notfier (channel) and events channel
    ///
    /// # Arguments
    /// * `transports_notifier` - The channel which works as a notifier about events in each transport. It's cloned for each new spawned transport
    ///
    fn process_server_events(
        &mut self,
        transports_notifier: UnboundedSender<TransportNotification<T>>,
    ) {
        let mut events_receiver = if let Some(events_receiver) = self.server_events_receiver.take()
        {
            events_receiver
        } else {
            panic!("> RpcServer > process_server_events > misuse of process_server_events, seems to be called more than one time")
        };

        tokio::spawn(async move {
            while let Some(event) = events_receiver.recv().await {
                match event {
                    ServerEvents::NewTransport(id, transport) => {
                        let tx_cloned = transports_notifier.clone();
                        tokio::spawn(async move {
                            loop {
                                match transport.receive().await {
                                    Ok(event) => {
                                        if tx_cloned
                                            .send(TransportNotification::NewMessage((
                                                (transport.clone(), id),
                                                event,
                                            )))
                                            .is_err()
                                        {
                                            error!("> From a Transport > Error while sending new message from transport to server via notifier");
                                            break;
                                        }
                                    }
                                    Err(error) => {
                                        error!(
                                            "> From a Transport > Error on receiving: {error:?}"
                                        );
                                        if matches!(error, TransportError::Closed) {
                                            if tx_cloned
                                                .send(TransportNotification::CloseTransport(id))
                                                .is_err()
                                            {
                                                error!("> From a Transport > Error while sending new message from transport to server via notifier");
                                                break;
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    ServerEvents::AttachTransport(transport) => {
                        if transports_notifier
                            .send(TransportNotification::MustAttachTransport(transport))
                            .is_err()
                        {
                            error!("> From a Transport > Error while notifying the server to attach a new transport");
                            continue;
                        };
                    }
                }
            }
        });
    }

    /// Set a handler for the port creation
    ///
    /// When a port is created, a service should be registered
    /// for the port.
    pub fn set_module_registrator_handler<H>(&mut self, handler: H)
    where
        H: Fn(&mut RpcServerPort<Context>) + Send + Sync + 'static,
    {
        self.port_creation_handler = Some(Box::new(handler));
    }

    /// Set a handler to be executed when a transport was closed
    ///
    /// When a transport closes its connection, the closure will be executed.
    ///
    /// This could be useful when there are resources that may be tied to or depends on a transport's connection
    pub fn set_on_transport_closes_handler<H>(&mut self, handler: H)
    where
        H: Fn(Arc<T>) + Send + Sync + 'static,
    {
        self.on_transport_closes_handler = Some(Box::new(handler));
    }

    /// Handle the requests for a procedure call
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport which sent the procedure request
    /// * `message_number` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_request(
        &self,
        transport: Arc<T>,
        message_number: u32,
        payload: Vec<u8>,
    ) -> ServerResult<()> {
        let request = Request::decode(payload.as_slice())
            .map_err(|_| ServerResultError::External(ServerError::ProtocolError))?;

        match self.ports.get(&request.port_id) {
            Some(port) => {
                let procedure_ctx = self.context.clone();
                let transport_cloned = transport.clone();
                let procedure_handler = port.get_procedure(request.procedure_id)?;

                match procedure_handler {
                    ProcedureDefinition::Unary(procedure_handler) => {
                        self.messages_handler.process_unary_request(
                            transport_cloned,
                            message_number,
                            procedure_handler(request.payload, procedure_ctx),
                        );
                    }
                    ProcedureDefinition::ServerStreams(procedure_handler) => {
                        self.messages_handler
                            // Cloned because the receiver of the function is an Arc. It'll be spawned in other thread and it needs to modify its state
                            .clone()
                            .process_server_streams_request(
                                transport_cloned,
                                message_number,
                                request.port_id,
                                procedure_handler(request.payload, procedure_ctx),
                            )
                    }
                    ProcedureDefinition::ClientStreams(procedure_handler) => {
                        let client_stream_id = request.client_stream;
                        let stream_protocol = StreamProtocol::new(
                            transport.clone(),
                            request.port_id,
                            request.client_stream,
                        );

                        let msg_handler = self.messages_handler.clone();
                        match stream_protocol
                            .start_processing(move || async move {
                                msg_handler.unregister_listener(client_stream_id).await
                            })
                            .await
                        {
                            Ok(listener) => {
                                self.messages_handler
                                    .clone()
                                    .process_client_streams_request(
                                        transport_cloned,
                                        message_number,
                                        client_stream_id,
                                        procedure_handler(
                                            stream_protocol.to_generator(Some),
                                            procedure_ctx,
                                        ),
                                        listener,
                                    );
                            }
                            Err(_) => {
                                return Err(ServerResultError::Internal(
                                    ServerInternalError::TransportError,
                                ))
                            }
                        }
                    }
                    ProcedureDefinition::BiStreams(procedure_handler) => {
                        let client_stream_id = request.client_stream;
                        let stream_protocol = StreamProtocol::new(
                            transport.clone(),
                            request.port_id,
                            request.client_stream,
                        );

                        let msg_handler = self.messages_handler.clone();
                        match stream_protocol
                            .start_processing(move || async move {
                                msg_handler.unregister_listener(client_stream_id).await
                            })
                            .await
                        {
                            Ok(listener) => {
                                self.messages_handler.clone().process_bidir_streams_request(
                                    transport_cloned,
                                    message_number,
                                    request.port_id,
                                    client_stream_id,
                                    listener,
                                    procedure_handler(
                                        stream_protocol.to_generator(Some),
                                        procedure_ctx,
                                    ),
                                );
                            }
                            Err(_) => {
                                return Err(ServerResultError::Internal(
                                    ServerInternalError::TransportError,
                                ))
                            }
                        }
                    }
                }

                Ok(())
            }
            _ => Err(ServerResultError::External(ServerError::PortNotFound)),
        }
    }

    /// Handle the requests when a client wants to load a specific registered module and then starts calling the procedures
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport which is requesting the module
    /// * `message_number` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_request_module(
        &mut self,
        transport: Arc<T>,
        message_number: u32,
        payload: Vec<u8>,
    ) -> ServerResult<()> {
        let request_module = RequestModule::decode(payload.as_slice())
            .map_err(|_| ServerResultError::External(ServerError::ProtocolError))?;
        if let Some(port) = self.ports.get_mut(&request_module.port_id) {
            if let Ok(server_module_declaration) = port.load_module(request_module.module_name) {
                let mut procedures: Vec<ModuleProcedure> = Vec::default();
                for procedure in &server_module_declaration.procedures {
                    let module_procedure = ModuleProcedure {
                        procedure_name: procedure.procedure_name.clone(),
                        procedure_id: procedure.procedure_id,
                    };
                    procedures.push(module_procedure)
                }

                let response = RequestModuleResponse {
                    port_id: request_module.port_id,
                    message_identifier: build_message_identifier(
                        RpcMessageTypes::RequestModuleResponse as u32,
                        message_number,
                    ),
                    procedures,
                };
                let response = response.encode_to_vec();
                transport
                    .send(response)
                    .await
                    .map_err(|_| ServerResultError::Internal(ServerInternalError::TransportError))?
            } else {
                return Err(ServerResultError::External(ServerError::LoadModuleError));
            }
        } else {
            return Err(ServerResultError::External(ServerError::PortNotFound));
        }

        Ok(())
    }

    /// Handle the requests when a client wants to create a port.
    ///
    /// The `handler` registered with `set_handler` function is called here.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport which sent the request to create a port
    /// * `message_number` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_create_port(
        &mut self,
        transport: Arc<T>,
        transport_id: TransportID,
        message_number: u32,
        payload: Vec<u8>,
    ) -> ServerResult<()> {
        let port_id = (self.ports.len() + 1) as u32;
        let create_port = CreatePort::decode(payload.as_slice())
            .map_err(|_| ServerResultError::External(ServerError::ProtocolError))?;
        let port_name = create_port.port_name;
        let mut port = RpcServerPort::new(port_name.clone());

        if let Some(handler) = &self.port_creation_handler {
            handler(&mut port);
        }

        self.ports.insert(port_id, port);
        self.ports_by_transport_id
            .entry(transport_id)
            .and_modify(|ports| ports.push(port_id))
            .or_insert_with(|| vec![port_id]);

        let response = CreatePortResponse {
            message_identifier: build_message_identifier(
                RpcMessageTypes::CreatePortResponse as u32,
                message_number,
            ),
            port_id,
        };
        let response = response.encode_to_vec();
        transport
            .send(response)
            .await
            .map_err(|_| ServerResultError::Internal(ServerInternalError::TransportError))?;

        Ok(())
    }

    /// Handle the requests when a client wants to destroy a port because no longer needed
    ///
    /// # Arguments
    ///
    /// * `payload` - Vec of bytes containing the request payload encoded with protobuf
    fn handle_destroy_port(&mut self, payload: Vec<u8>) -> ServerResult<()> {
        let destroy_port = DestroyPort::decode(payload.as_slice())
            .map_err(|_| ServerResultError::External(ServerError::ProtocolError))?;

        self.ports.remove(&destroy_port.port_id);
        Ok(())
    }

    /// Handle every request from the client.
    ///
    /// Then, parse the "header" that contains the `message_type` and `message_identifier`
    ///
    /// This allows us know which function should finially handle the request
    ///
    /// # Arguments
    ///
    /// * `transport_id` - The transport ID which sent a new message to be processed
    /// * `payload` - Vec of bytes containing the request payload encoded with protobuf
    /// * `message_type` - [`RpcMessageTypes`] the protocol type of the message
    /// * `message_number` - the number of the message derivided from the `message_identifier` in the [`crate::rpc_protocol::RpcMessageHeader`]
    async fn handle_message(
        &mut self,
        transport_id: TransportID,
        payload: Vec<u8>,
        message_type: RpcMessageTypes,
        message_number: u32,
    ) -> ServerResult<()> {
        let transport = self
            .transports
            .get(&transport_id)
            .ok_or(ServerResultError::Internal(
                ServerInternalError::TransportNotAttached,
            ))?
            .clone();
        match message_type {
            RpcMessageTypes::Request => {
                self.handle_request(transport, message_number, payload)
                    .await?
            }
            RpcMessageTypes::RequestModule => {
                self.handle_request_module(transport, message_number, payload)
                    .await?
            }
            RpcMessageTypes::CreatePort => {
                self.handle_create_port(transport, transport_id, message_number, payload)
                    .await?
            }
            RpcMessageTypes::DestroyPort => self.handle_destroy_port(payload)?,
            RpcMessageTypes::StreamAck => {
                // Client akcnowledged a stream message sent by Server
                // and we should notify the waiter for the ack in order to
                // continue sending streams to Client
                self.messages_handler
                    .streams_handler
                    .clone()
                    .message_acknowledged_by_peer(message_number, payload)
            }
            RpcMessageTypes::StreamMessage => {
                // Client has a client stream request type opened and we should
                // notify our listener for the client message id that we have a new message to process
                self.messages_handler
                    .clone()
                    .notify_new_client_stream(message_number, payload)
            }
            _ => {
                debug!("Unknown message");
            }
        };

        Ok(())
    }
}

/// RpcServerPort is what a RpcServer contains to handle different services/modules
pub struct RpcServerPort<Context> {
    /// RpcServer name
    pub name: String,
    /// Registered modules contains the name and module/service definition
    ///
    /// A module can be registered but not loaded
    registered_modules: HashMap<String, ServiceModuleDefinition<Context>>,
    /// Loaded modules contains the name and a collection of procedures with id and the name for each one
    ///
    /// A module is loaded when the client requests to.
    loaded_modules: HashMap<String, ServerModuleDeclaration>,
    /// Procedures contains the id and the handler for each procedure
    procedures: HashMap<u32, ProcedureDefinition<Context>>,
}

impl<Context> RpcServerPort<Context> {
    fn new(name: String) -> Self {
        RpcServerPort {
            name,
            registered_modules: HashMap::new(),
            loaded_modules: HashMap::new(),
            procedures: HashMap::new(),
        }
    }

    /// Just register the module in the port
    pub fn register_module(
        &mut self,
        module_name: String,
        service_definition: ServiceModuleDefinition<Context>,
    ) {
        self.registered_modules
            .insert(module_name, service_definition);
    }

    /// It checks if the module is already loaded and return it.
    ///
    /// Otherwise, it will get the module definition from the `registered_modules` and load it
    fn load_module(&mut self, module_name: String) -> ServerResult<&ServerModuleDeclaration> {
        if self.loaded_modules.contains_key(&module_name) {
            Ok(self
                .loaded_modules
                .get(&module_name)
                .expect("Already checked."))
        } else {
            match self.registered_modules.get(&module_name) {
                None => Err(ServerResultError::External(ServerError::ModuleNotFound)),
                Some(module_generator) => {
                    let mut server_module_declaration = ServerModuleDeclaration {
                        procedures: Vec::new(),
                    };

                    let definitions = module_generator.get_definitions();
                    let mut procedure_id = 1;

                    for (procedure_name, procedure_definition) in definitions {
                        let current_id = procedure_id;
                        self.procedures
                            .insert(current_id, procedure_definition.clone());
                        server_module_declaration
                            .procedures
                            .push(ServerModuleProcedure {
                                procedure_name: procedure_name.clone(),
                                procedure_id: current_id,
                            });
                        procedure_id += 1
                    }

                    self.loaded_modules
                        .insert(module_name.clone(), server_module_declaration);

                    let module_definition = self
                        .loaded_modules
                        .get(&module_name)
                        .ok_or(ServerResultError::External(ServerError::LoadModuleError))?;
                    Ok(module_definition)
                }
            }
        }
    }

    /// It will look up the procedure id in the port's `procedures` and return the procedure's handler
    fn get_procedure(&self, procedure_id: u32) -> ServerResult<ProcedureDefinition<Context>> {
        match self.procedures.get(&procedure_id) {
            Some(procedure_definition) => Ok(procedure_definition.clone()),
            _ => Err(ServerResultError::External(ServerError::ProcedureNotFound)),
        }
    }
}

#[derive(Debug)]
pub struct ServerModuleProcedure {
    pub procedure_name: String,
    pub procedure_id: u32,
}

/// Used to store all the procedures in the `loaded_modules` fields inside [`RpcServerPort`]
pub struct ServerModuleDeclaration {
    /// Array with all the module's (service) procedures
    pub procedures: Vec<ServerModuleProcedure>,
}
