use std::{collections::HashMap, sync::Arc, u8};

use log::{debug, error};
use prost::{alloc::vec::Vec, Message};

use crate::{
    messages_handlers::ServerMessagesHandler,
    protocol::{
        parse::{build_message_identifier, parse_header},
        CreatePort, CreatePortResponse, DestroyPort, ModuleProcedure, Request, RequestModule,
        RequestModuleResponse, RpcMessageTypes,
    },
    stream_protocol::StreamProtocol,
    transports::{Transport, TransportEvent},
    types::{
        ClientStreamsRequestHandler, Definition, ServerModuleDeclaration, ServerModuleProcedures,
        ServerStreamsRequestHandler, ServiceModuleDefinition, UnaryRequestHandler,
    },
};

type PortHandlerFn<Context> = dyn Fn(&mut RpcServerPort<Context>) + Send + Sync + 'static;

pub type ServerResult<T> = Result<T, ServerError>;

enum Procedure<Context> {
    Unary(Arc<UnaryRequestHandler<Context>>),
    ServerStreams(Arc<ServerStreamsRequestHandler<Context>>),
    ClientStreams(Arc<ClientStreamsRequestHandler<Context>>),
}

#[derive(Debug)]
pub enum ServerError {
    ProtocolError,
    TransportError,
    TransportNotAttached,
    PortNotFound,
    LoadModuleError,
    ModuleNotAvailable,
    RegisterModuleError,
    UnknownMessage,
    InvalidHeader,
    ProcedureError,
}

/// RpcServer receives and process different requests from the RpcClient
///
/// Once a RpcServer is inited, you should attach a transport and handler
/// for the port creation.
pub struct RpcServer<Context> {
    /// The Transport used for the communication between `RpcClient` and `RpcServer`
    transport: Option<Arc<dyn Transport + Send + Sync>>,
    /// The handler executed when a new port is created
    handler: Option<Box<PortHandlerFn<Context>>>,
    /// Ports registered in the `RpcServer`
    ports: HashMap<u32, RpcServerPort<Context>>,
    /// RpcServer Context
    context: Arc<Context>,
    /// Handler in charge of handling every request<>response.
    ///
    /// It's stored inside an `Arc` because it'll be shared between threads
    messages_handler: Arc<ServerMessagesHandler>,
}
impl<Context: Send + Sync + 'static> RpcServer<Context> {
    pub fn create(ctx: Context) -> Self {
        Self {
            transport: None,
            handler: None,
            ports: HashMap::new(),
            context: Arc::new(ctx),
            messages_handler: Arc::new(ServerMessagesHandler::new()),
        }
    }

    /// Attach the server half of the transport for Client<>Server comms
    pub fn attach_transport<T: Transport + Send + Sync + 'static>(&mut self, transport: T) {
        self.transport = Some(Arc::new(transport));
    }

    /// Start listening messages from the attached transport
    pub async fn run(&mut self) {
        loop {
            match self
                .transport
                .as_mut()
                .expect("No transport attached")
                .receive()
                .await
            {
                Ok(event) => match event {
                    TransportEvent::Connect => {
                        // Response back to the client to finally establish the connection
                        // on both ends
                        self.transport
                            .as_ref()
                            .unwrap()
                            .send(vec![0])
                            .await
                            .expect("expect to be able to connect");
                    }
                    TransportEvent::Error(err) => error!("Transport error {}", err),
                    TransportEvent::Message(payload) => match self.handle_message(payload).await {
                        Ok(_) => debug!("Transport message handled!"),
                        Err(e) => error!("Failed to handle message: {:?}", e),
                    },
                    TransportEvent::Close => {
                        error!("Transport closed");
                        break;
                    }
                },
                Err(_) => {
                    error!("Transport error");
                    break;
                }
            }
        }
    }

    /// Set a handler for the port creation
    ///
    /// When a port is created, a service should be registered
    /// for the port.
    pub fn set_handler<H>(&mut self, handler: H)
    where
        H: Fn(&mut RpcServerPort<Context>) + Send + Sync + 'static,
    {
        self.handler = Some(Box::new(handler));
    }

    /// Handle the requests for a procedure call
    ///
    /// # Arguments
    ///
    /// * `message_identifier` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_request(&self, message_identifier: u32, payload: &[u8]) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let request = Request::decode(payload).map_err(|_| ServerError::ProtocolError)?;

        match self.ports.get(&request.port_id) {
            Some(port) => {
                let procedure_ctx = self.context.clone();
                let transport_cloned = transport.clone();
                let procedure_handler = port.get_procedure(request.procedure_id)?;

                match procedure_handler {
                    Procedure::Unary(procedure_handler) => {
                        self.messages_handler.process_unary_request(
                            transport_cloned,
                            message_identifier,
                            procedure_handler(request.payload, procedure_ctx),
                        );
                    }
                    Procedure::ServerStreams(procedure_handler) => {
                        self.messages_handler
                            // Cloned because the receiver of the function is an Arc. It'll be spawned in other thread and it needs to modify its state
                            .clone()
                            .process_server_streams_request(
                                transport_cloned,
                                message_identifier,
                                request.port_id,
                                procedure_handler(request.payload, procedure_ctx),
                            )
                    }
                    Procedure::ClientStreams(procedure_handler) => {
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
                                        message_identifier,
                                        client_stream_id,
                                        procedure_handler(
                                            stream_protocol.to_generator(|item| item),
                                            procedure_ctx,
                                        ),
                                        listener,
                                    );
                            }
                            Err(_) => return Err(ServerError::TransportError),
                        }
                    }
                }

                Ok(())
            }
            _ => Err(ServerError::PortNotFound),
        }
    }

    /// Handle the requests when a client wants to load a specific registered module and then starts calling the procedures
    ///
    /// # Arguments
    ///
    /// * `message_identifier` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_request_module(
        &mut self,
        message_identifier: u32,
        payload: &[u8],
    ) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let request_module =
            RequestModule::decode(payload).map_err(|_| ServerError::ProtocolError)?;
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
                        message_identifier,
                    ),
                    procedures,
                };
                let response = response.encode_to_vec();
                transport
                    .send(response)
                    .await
                    .map_err(|_| ServerError::TransportError)?
            } else {
                return Err(ServerError::LoadModuleError);
            }
        } else {
            return Err(ServerError::PortNotFound);
        }

        Ok(())
    }

    /// Handle the requests when a client wants to create a port.
    ///
    /// The `handler` registered with `set_handler` function is called here.
    ///
    /// # Arguments
    ///
    /// * `message_identifier` - A 32-bit unsigned number created by `build_message_identifier` in `protocol/parse.rs`
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    async fn handle_create_port(
        &mut self,
        message_identifier: u32,
        payload: &[u8],
    ) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let port_id = (self.ports.len() + 1) as u32;
        let create_port = CreatePort::decode(payload).map_err(|_| ServerError::ProtocolError)?;
        let port_name = create_port.port_name;
        let mut port = RpcServerPort::new(port_name.clone());

        if let Some(handler) = &self.handler {
            handler(&mut port);
        }

        self.ports.insert(port_id, port);

        let response = CreatePortResponse {
            message_identifier: build_message_identifier(
                RpcMessageTypes::CreatePortResponse as u32,
                message_identifier,
            ),
            port_id,
        };
        let response = response.encode_to_vec();
        transport
            .send(response)
            .await
            .map_err(|_| ServerError::TransportError)?;

        Ok(())
    }

    /// Handle the requests when a client wants to destroy a port because no longer needed
    ///
    /// # Arguments
    ///
    /// * `payload` - Slice of bytes containing the request payload encoded with protobuf
    fn handle_destroy_port(&mut self, payload: &[u8]) -> ServerResult<()> {
        let destroy_port = DestroyPort::decode(payload).map_err(|_| ServerError::ProtocolError)?;

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
    /// * `payload` - Vec of bytes containing the request payload encoded with protobuf
    async fn handle_message(&mut self, payload: Vec<u8>) -> ServerResult<()> {
        let (message_type, message_identifier) =
            parse_header(&payload).ok_or(ServerError::ProtocolError)?;

        match message_type {
            RpcMessageTypes::Request => self.handle_request(message_identifier, &payload).await?,
            RpcMessageTypes::RequestModule => {
                self.handle_request_module(message_identifier, &payload)
                    .await?
            }
            RpcMessageTypes::CreatePort => {
                self.handle_create_port(message_identifier, &payload)
                    .await?
            }
            RpcMessageTypes::DestroyPort => self.handle_destroy_port(&payload)?,
            RpcMessageTypes::StreamAck => {
                // Client akcnowledged a stream message sent by Server
                // and we should notify the waiter for the ack in order to
                // continue sending streams to Client
                self.messages_handler
                    .streams_handler
                    .clone()
                    .message_acknowledged_by_peer(message_identifier, payload)
            }
            RpcMessageTypes::StreamMessage => {
                // Client has a client stream request type opened and we should
                // notify our listener for the client message id that we have a new message to process
                self.messages_handler
                    .clone()
                    .notify_new_client_stream(message_identifier, payload)
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
    procedures: HashMap<u32, Definition<Context>>,
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
                None => Err(ServerError::ModuleNotAvailable),
                Some(module_generator) => {
                    let mut server_module_declaration = ServerModuleDeclaration {
                        procedures: Vec::new(),
                    };

                    let definitions = module_generator.get_definitions();
                    let mut procedure_id = 1;

                    for (procedure_name, procedure_definition) in definitions {
                        let current_id = procedure_id;
                        match procedure_definition {
                            Definition::Unary(procedure) => self
                                .procedures
                                .insert(current_id, Definition::Unary(procedure.clone())),
                            Definition::ServerStreams(procedure) => self
                                .procedures
                                .insert(current_id, Definition::ServerStreams(procedure.clone())),
                            &Definition::ClientStreams(ref procedure) => self
                                .procedures
                                .insert(current_id, Definition::ClientStreams(procedure.clone())),
                        };
                        server_module_declaration
                            .procedures
                            .push(ServerModuleProcedures {
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
                        .ok_or(ServerError::LoadModuleError)?;
                    Ok(module_definition)
                }
            }
        }
    }

    /// It will look up the procedure id in the port's `procedures` and return the procedure's handler
    fn get_procedure(&self, procedure_id: u32) -> ServerResult<Procedure<Context>> {
        match self.procedures.get(&procedure_id) {
            Some(procedure_definition) => match procedure_definition {
                Definition::Unary(procedure_handler) => {
                    return Ok(Procedure::Unary(procedure_handler.clone()));
                }
                Definition::ServerStreams(procedure_handler) => {
                    return Ok(Procedure::ServerStreams(procedure_handler.clone()));
                }
                Definition::ClientStreams(procedure_handler) => {
                    return Ok(Procedure::ClientStreams(procedure_handler.clone()));
                }
            },
            _ => Err(ServerError::ProcedureError),
        }
    }
}
