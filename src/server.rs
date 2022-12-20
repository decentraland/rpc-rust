use std::{collections::HashMap, sync::Arc, u8};

use protobuf::{Message, ProtobufError, RepeatedField};

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
                Ok(event) => {
                    if let TransportEvent::Connect = event {
                        println!("Transport connected");
                    } else if let TransportEvent::Message(payload) = event {
                        println!("Transport new data");
                        self.handle_message(payload, &mut ports)
                            .await
                            .expect("can handle message");
                    } else if let TransportEvent::Error(err) = event {
                        println!("Transport error {}", err);
                    } else if let TransportEvent::Close = event {
                        println!("Transport closed");
                        break;
                    }
                }
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

    async fn handle_request<T: Transport + ?Sized>(
        &self,
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
        transport: impl AsRef<T>,
    ) -> Result<(), ProtobufError> {
        let request = Request::parse_from_bytes(payload)?;
        print!("Request {:?}", request);
        if let Some(port) = ports.get(&request.port_id) {
            let procedure_response = port.call_procedure(request.procedure_id, request.payload);

            let response = Response {
                message_identifier,
                payload: procedure_response,
                ..Default::default()
            };

            transport
                .as_ref()
                .send(response.write_to_bytes()?)
                .await
                .expect("message to be sent");
        } else {
            println!("> REQUEST > no port for given id ");
        }
        Ok(())
    }

    async fn handle_request_module<T: Transport + ?Sized>(
        &self,
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
        transport: impl AsRef<T>,
    ) -> Result<(), ProtobufError> {
        let request_module = RequestModule::parse_from_bytes(payload)?;
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
                transport
                    .as_ref()
                    .send(response.write_to_bytes()?)
                    .await
                    .expect("message to be sent")
            } else {
                println!("> REQUEST_MODULE > unable to load the module")
            }
        } else {
            println!("> REQUEST_MODULE > unable to get the port")
        }

        Ok(())
    }

    async fn handle_create_port<T: Transport + ?Sized>(
        &self, 
        message_identifier: u32,
        payload: &[u8],
        ports: &mut HashMap<u32, RpcServerPort>,
        transport: impl AsRef<T>,
    ) -> Result<(), ProtobufError> {
        let port_id = (ports.len() + 1) as u32;
        let create_port = CreatePort::parse_from_bytes(payload)?;
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
        transport.as_ref()
            .send(response.write_to_bytes()?)
            .await
            .expect("message to be sent");

        Ok(())
    }

    async fn handle_message(
        &self,
        payload: Vec<u8>,
        ports: &mut HashMap<u32, RpcServerPort>,
    ) -> Result<(), ProtobufError> {
        let transport = self.transport.as_ref().expect("Transport not attached");
        let header = parse_header(&payload);

        match header {
            Some((message_type, message_identifier)) => {
                match message_type {
                    RpcMessageTypes::RpcMessageTypes_REQUEST => {
                        self.handle_request(message_identifier, &payload, ports, transport).await?
                    }
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
                        self.handle_request_module(message_identifier, &payload, ports, transport)
                            .await?
                    }
                    RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
                        self.handle_create_port(message_identifier, &payload, ports, transport)
                            .await?
                    }
                    RpcMessageTypes::RpcMessageTypes_DESTROY_PORT => {
                        let destroy_port = DestroyPort::parse_from_bytes(&payload)?;
                        print!("DestroyPort {:?}", destroy_port);
                    }
                    RpcMessageTypes::RpcMessageTypes_STREAM_ACK
                    | RpcMessageTypes::RpcMessageTypes_STREAM_MESSAGE => {
                        // noops
                    }
                    _ => {
                        println!("Unknown message");
                    }
                }
            }
            None => {
                println!("Invalid header");
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

    fn load_module(&mut self, module_name: String) -> Result<&ServerModuleDeclaration, String> {
        if self.loaded_modules.get(&module_name).is_some() {
            Ok(self.loaded_modules.get(&module_name).unwrap())
        } else {
            let module_generator = self.registered_modules.get(&module_name);
            if module_generator.is_none() {
                Err(String::from(
                    "the module requested is not avaialable for the port",
                ))
            } else {
                let mut server_module_declaration = ServerModuleDeclaration {
                    procedures: Vec::new(),
                };

                let definitions = module_generator.unwrap().get_definitions();
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

                Ok(self.loaded_modules.get(&module_name).unwrap())
            }
        }
    }

    pub fn call_procedure(&self, procedure_id: u32, payload: Vec<u8>) -> Vec<u8> {
        if self.procedures.get(&procedure_id).is_some() {
            let handler = self.procedures.get(&procedure_id).unwrap();
            handler(&payload)
        } else {
            println!("Error on calling procedure");
            Vec::new()
        }
    }
}
