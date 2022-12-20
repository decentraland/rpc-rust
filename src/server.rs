use std::{collections::HashMap, sync::Arc, u8};

use protobuf::{Message, RepeatedField};

use crate::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, DestroyPort, ModuleProcedure, Request, RequestModule,
            RequestModuleResponse, Response, RpcMessageTypes,
        },
        parse::parse_header,
    },
    transports::{Transport, TransportEvent},
    types::{
        ServerModuleDeclaration, ServerModuleProcedures, ServiceModuleDefinition,
        UnaryRequestHandler,
    },
};

type PortHandlerFn = dyn Fn(&mut RpcServerPort) + Send + Sync + 'static;

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

pub struct RpcServer {
    transport: Option<Box<dyn Transport + Send + Sync>>,
    handler: Option<Box<PortHandlerFn>>,
}
impl RpcServer {
    pub fn create() -> Self {
        Self {
            transport: None,
            handler: None,
        }
    }

    pub fn attach_transport<T: Transport + Send + Sync + 'static>(&mut self, transport: T) {
        self.transport = Some(Box::new(transport));
    }

    pub async fn run(&mut self) {
        let mut ports: HashMap<u32, RpcServerPort> = HashMap::new();
        loop {
            match self
                .transport
                .as_mut()
                .expect("No transport attached")
                .receive()
                .await
            {
                Ok(event) => match event {
                    TransportEvent::Connect => println!("Transport connected"),
                    TransportEvent::Error(err) => println!("Transport error {}", err),
                    TransportEvent::Message(payload) => {
                        match self.handle_message(payload, &mut ports).await {
                            Ok(_) => println!("Transport message handled!"),
                            Err(e) => println!("Failed to handle message: {:?}", e),
                        }
                    }
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

    pub fn set_handler<H>(&mut self, handler: H)
    where
        H: Fn(&mut RpcServerPort) + Send + Sync + 'static,
    {
        self.handler = Some(Box::new(handler));
    }

    async fn handle_request(
        &self,
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let request = Request::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;
        println!("Request {:?}", request);

        match ports.get(&request.port_id) {
            Some(port) => {
                let procedure_response =
                    port.call_procedure(request.procedure_id, request.payload)?;

                let response = Response {
                    message_identifier,
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

    async fn handle_request_module(
        &self,
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let request_module =
            RequestModule::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;
        println!("> REQUEST_MODULE > request: {:?}", request_module);
        if let Some(port) = ports.get_mut(&request_module.port_id) {
            if let Ok(server_module_declaration) = port.load_module(request_module.module_name) {
                let mut response = RequestModuleResponse::default();
                response.set_port_id(request_module.port_id);
                response.set_message_identifier(message_identifier);
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
                println!("> REQUEST_MODULE > unable to load the module")
            }
        } else {
            println!("> REQUEST_MODULE > unable to get the port")
        }

        Ok(())
    }

    async fn handle_create_port(
        &self,
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> ServerResult<()> {
        let transport = self
            .transport
            .as_ref()
            .ok_or(ServerError::TransportNotAttached)?;
        let port_id = (ports.len() + 1) as u32;
        let create_port =
            CreatePort::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;
        println!("CreatePort {:?}", create_port);
        let port_name = create_port.port_name;
        let mut port = RpcServerPort::new(port_name.clone());

        if let Some(handler) = &self.handler {
            handler(&mut port);
        }

        ports.insert(port_id, port);

        let mut response = CreatePortResponse::new();
        response.message_identifier = message_identifier;
        response.port_id = port_id;
        let response = response
            .write_to_bytes()
            .map_err(|_| ServerError::TransportError)?;
        transport
            .send(response)
            .await
            .map_err(|_| ServerError::TransportError)?;

        Ok(())
    }

    fn handle_destroy_port(
        &self,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> ServerResult<()> {
        let destroy_port =
            DestroyPort::parse_from_bytes(payload).map_err(|_| ServerError::ProtocolError)?;

        println!("DestroyPort {:?}", destroy_port);

        ports.remove(&destroy_port.port_id);
        Ok(())
    }

    async fn handle_message(
        &self,
        payload: Vec<u8>,
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> ServerResult<()> {
        let (message_type, message_identifier) =
            parse_header(&payload).ok_or(ServerError::ProtocolError)?;

        match message_type {
            RpcMessageTypes::RpcMessageTypes_REQUEST => {
                self.handle_request(message_identifier, &payload, ports)
                    .await?
            }
            RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
                self.handle_request_module(message_identifier, &payload, ports)
                    .await?
            }
            RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
                self.handle_create_port(message_identifier, &payload, ports)
                    .await?
            }
            RpcMessageTypes::RpcMessageTypes_DESTROY_PORT => {
                self.handle_destroy_port(&payload, ports)?
            }
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
pub struct RpcServerPort {
    pub name: String,
    registered_modules: HashMap<String, ServiceModuleDefinition>,
    loaded_modules: HashMap<String, ServerModuleDeclaration>,
    procedures: HashMap<u32, Arc<Box<UnaryRequestHandler>>>,
}

impl RpcServerPort {
    pub fn new(name: String) -> Self {
        RpcServerPort {
            name,
            registered_modules: HashMap::new(),
            loaded_modules: HashMap::new(),
            procedures: HashMap::new(),
        }
    }

    pub fn register_module(
        &mut self,
        module_name: String,
        service_definition: ServiceModuleDefinition,
    ) {
        self.registered_modules
            .insert(module_name, service_definition);
    }

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

    pub fn call_procedure(&self, procedure_id: u32, payload: Vec<u8>) -> ServerResult<Vec<u8>> {
        match self.procedures.get(&procedure_id) {
            Some(procedure_handler) => Ok(procedure_handler(&payload)),
            _ => Err(ServerError::ProcedureError),
        }
    }
}
