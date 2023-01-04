use std::{collections::HashMap, sync::Arc, u8};

use protobuf::{Message, RepeatedField};

use crate::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, DestroyPort, ModuleProcedure, Request, RequestModule,
            RequestModuleResponse, Response, RpcMessageTypes,
        },
        parse::{build_message_identifier, parse_header},
    },
    transports::{Transport, TransportEvent},
    types::{
        ServerModuleDeclaration, ServerModuleProcedures, ServiceModuleDefinition,
        UnaryRequestHandler,
    },
};

type PortHandlerFn<Context> = dyn Fn(&mut RpcServerPort<Context>) + Send + Sync + 'static;

pub type ServerResult<T> = Result<T, ServerError>;

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
    /// The Transport used for the communicatio between `RpcClient` and `RpcServer`
    transport: Option<Box<dyn Transport + Send + Sync>>,
    /// The handler executed when a new port is created
    handler: Option<Box<PortHandlerFn<Context>>>,
    /// Ports registered in the `RpcServer`
    ports: HashMap<u32, RpcServerPort<Context>>,
    /// RpcServer Context
    context: Arc<Context>,
}
impl<Context> RpcServer<Context> {
    pub fn create(ctx: Context) -> Self {
        Self {
            transport: None,
            handler: None,
            ports: HashMap::new(),
            context: Arc::new(ctx),
        }
    }

    /// Attach the server half of the transport for Client<>Server comms
    pub fn attach_transport<T: Transport + Send + Sync + 'static>(&mut self, transport: T) {
        self.transport = Some(Box::new(transport));
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
                        self.transport
                            .as_mut()
                            .expect("No transport attached")
                            .connected()
                            .await;
                        println!("Transport connected");
                        // Response back to the client to finally establish the connection
                        // on both ends
                        self.transport
                            .as_ref()
                            .unwrap()
                            .send(vec![0])
                            .await
                            .expect("expect to be able to connect");
                    }
                    TransportEvent::Error(err) => println!("Transport error {}", err),
                    TransportEvent::Message(payload) => match self.handle_message(payload).await {
                        Ok(_) => println!("Transport message handled!"),
                        Err(e) => println!("Failed to handle message: {:?}", e),
                    },
                    TransportEvent::Close => {
                        println!("Transport closed");
                        break;
                    }
                },
                Err(_) => {
                    println!("Transport error");
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
        let request = Request::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;

        match self.ports.get(&request.port_id) {
            Some(port) => {
                let procedure_ctx = self.context.clone();
                let procedure_response = port
                    .call_procedure(request.procedure_id, request.payload, procedure_ctx)
                    .await?;

                let response = Response {
                    message_identifier: build_message_identifier(
                        RpcMessageTypes::RpcMessageTypes_RESPONSE as u32,
                        message_identifier,
                    ),
                    payload: procedure_response,
                    ..Default::default()
                };

                transport
                    .send(
                        response
                            .write_to_bytes()
                            .map_err(|_| ServerError::TransportError)?,
                    )
                    .await
                    .map_err(|_| ServerError::TransportError)?;
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
            RequestModule::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;
        if let Some(port) = self.ports.get_mut(&request_module.port_id) {
            if let Ok(server_module_declaration) = port.load_module(request_module.module_name) {
                let mut response = RequestModuleResponse::default();
                response.set_port_id(request_module.port_id);
                response.set_message_identifier(build_message_identifier(
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE_RESPONSE as u32,
                    message_identifier,
                ));
                let mut procedures: RepeatedField<ModuleProcedure> = RepeatedField::default();
                for procedure in &server_module_declaration.procedures {
                    let mut module_procedure = ModuleProcedure::default();
                    module_procedure.set_procedure_id(procedure.procedure_id);
                    module_procedure.set_procedure_name(procedure.procedure_name.clone());
                    procedures.push(module_procedure)
                }
                response.set_procedures(procedures);
                let response = response
                    .write_to_bytes()
                    .map_err(|_| ServerError::TransportError)?;
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
        let create_port =
            CreatePort::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;
        let port_name = create_port.port_name;
        let mut port = RpcServerPort::new(port_name.clone());

        if let Some(handler) = &self.handler {
            handler(&mut port);
        }

        self.ports.insert(port_id, port);

        let response = CreatePortResponse {
            message_identifier: build_message_identifier(
                RpcMessageTypes::RpcMessageTypes_CREATE_PORT_RESPONSE as u32,
                message_identifier,
            ),
            port_id,
            ..Default::default()
        };
        let response = response
            .write_to_bytes()
            .map_err(|_| ServerError::TransportError)?;
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
        let destroy_port =
            DestroyPort::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;

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
            RpcMessageTypes::RpcMessageTypes_REQUEST => {
                self.handle_request(message_identifier, &payload).await?
            }
            RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
                self.handle_request_module(message_identifier, &payload)
                    .await?
            }
            RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
                self.handle_create_port(message_identifier, &payload)
                    .await?
            }
            RpcMessageTypes::RpcMessageTypes_DESTROY_PORT => self.handle_destroy_port(&payload)?,
            RpcMessageTypes::RpcMessageTypes_STREAM_ACK
            | RpcMessageTypes::RpcMessageTypes_STREAM_MESSAGE => {
                // noops
            }
            _ => {
                println!("Unknown message");
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
    procedures: HashMap<u32, Arc<UnaryRequestHandler<Context>>>,
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

                    for def in definitions {
                        let current_id = procedure_id;
                        self.procedures.insert(current_id, def.1.clone());
                        server_module_declaration
                            .procedures
                            .push(ServerModuleProcedures {
                                procedure_name: def.0.clone(),
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

    /// It will look up the procedure id in the port's `procedures` and execute the procedure's handler
    async fn call_procedure(
        &self,
        procedure_id: u32,
        payload: Vec<u8>,
        context: Arc<Context>,
    ) -> ServerResult<Vec<u8>> {
        match self.procedures.get(&procedure_id) {
            Some(procedure_handler) => Ok(procedure_handler(payload, context).await),
            _ => Err(ServerError::ProcedureError),
        }
    }
}
