use protobuf::Message;

use crate::{
    protocol::{
        index::{CreatePort, DestroyPort, Request, RequestModule, RpcMessageTypes},
        parse::parse_header,
    },
    transports::{Transport, TransportEvent},
};

pub struct RpcServer {
    transport: Option<Box<dyn Transport>>,
    handler: Option<Box<dyn Fn(RpcServerPort)>>,
}

pub struct RpcServerPort {}

impl RpcServer {
    // TODO: allow multiple transports
    pub fn create() -> Self {
        Self {
            transport: None,
            handler: None,
        }
    }

    pub fn attach_transport<T: Transport + 'static>(&mut self, transport: T) {
        self.transport = Some(Box::new(transport));
    }

    pub async fn run(&mut self) {
        loop {
            match self.transport.as_mut().expect("No transport attached").receive().await {
                Ok(event) => {
                    if let TransportEvent::Connect = event {
                        println!("Transport connected");
                    } else if let TransportEvent::Message(payload) = event {
                        println!("Transport new data");
                        self.handle_message(payload);
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

    pub fn set_handler(&mut self, handler: Box<dyn Fn(RpcServerPort) + 'static + Send>) {
        self.handler = Some(handler);
    }

    fn handle_message(&self, payload: Vec<u8>) {
        let transport = self.transport.as_ref().expect("Transport not attached");
        let header = parse_header(&payload);

        match header {
            Some((message_type, message_number)) => {
                match message_type {
                    RpcMessageTypes::RpcMessageTypes_REQUEST => {
                        let request = Request::parse_from_bytes(&payload);
                        print!("Request");
                    }
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
                        let request_module = RequestModule::parse_from_bytes(&payload);
                        print!("RequestModule");
                    }
                    RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
                        let create_port = CreatePort::parse_from_bytes(&payload);
                        print!("CreatePort");
                    }
                    RpcMessageTypes::RpcMessageTypes_DESTROY_PORT => {
                        let destroy_port = DestroyPort::parse_from_bytes(&payload);
                        print!("DestroyPort");
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
        }
    }
}

impl RpcServerPort {
    fn load_module(&self) {
        todo!()
    }
}
