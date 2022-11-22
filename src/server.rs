use std::collections::HashMap;

use protobuf::{Message, ProtobufError};

use crate::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, DestroyPort, Request, RequestModule, RpcMessageTypes,
        },
        parse::parse_header,
    },
    transports::{Transport, TransportEvent},
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
                        let request = Request::parse_from_bytes(&payload)?;
                        if let Some(port) = ports.get(&request.port_id) {}
                        print!("Request {:?}", request);
                    }
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
                        let request_module = RequestModule::parse_from_bytes(&payload)?;
                        print!("RequestModule {:?}", request_module);
                    }
                    RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
                        let port_id = (ports.len() + 1) as u32;
                        let create_port = CreatePort::parse_from_bytes(&payload)?;
                        print!("CreatePort {:?}", create_port);
                        let port_name = create_port.port_name;
                        let mut port = RpcServerPort::new(port_name.clone());

                        if let Some(handler) = &self.handler {
                            handler(&mut port);
                        }

                        ports.insert(port_id, port);

                        let mut response = CreatePortResponse::new();
                        response.message_identifier = message_identifier;
                        response.port_id = port_id;
                        transport
                            .send(response.write_to_bytes()?)
                            .await
                            .expect("message to be sent");
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
type RequestHandler = dyn Fn(&[u8]) + Send + Sync;
pub struct RpcServerPort {
    pub name: String,
    pub handlers: HashMap<String, Box<RequestHandler>>,
}

impl RpcServerPort {
    pub fn new(name: String) -> Self {
        RpcServerPort {
            name,
            handlers: HashMap::new(),
        }
    }
    pub fn register<H: Fn(&[u8]) + Send + Sync + 'static>(&mut self, name: String, handler: H) {
        self.handlers.insert(name, Box::new(handler));
    }
    fn load_module(&self) {
        todo!()
    }
}
