use std::{collections::HashMap, sync::Arc};

use futures_util::TryFutureExt;
use protobuf::Message;
use tokio::sync::RwLock;

use crate::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, Request, RequestModule, RequestModuleResponse,
            Response, RpcMessageTypes,
        },
        parse::{build_message_identifier, parse_protocol_message},
    },
    transports::{Transport, TransportEvent},
};

pub type ClientResult<T> = Result<T, ClientError>;

#[derive(Debug)]
pub enum ClientError {
    ProtocolError,
    TransportError,
    TransportNotAttached,
    PortCreationError,
    ProcedureNotFound,
    UnknownError,
}

pub struct RpcClient {
    ports: HashMap<String, RpcClientPort>,
    client_request_dispatcher: Arc<ClientRequestDispatcher>,
}

impl RpcClient {
    pub async fn new<T: Transport + Send + Sync + 'static>(transport: T) -> ClientResult<Self> {
        println!("Client IsConnected: {}", transport.is_connected());
        let transport = Self::establish_connection(transport)
            .map_err(|err| err)
            .await?;
        let transport = Arc::new(transport);

        println!("Client IsConnected: {}", transport.is_connected());

        let client = Self {
            ports: HashMap::new(),
            client_request_dispatcher: Arc::new(ClientRequestDispatcher::new(transport)),
        };
        Ok(client)
    }

    async fn establish_connection<T: Transport + Send + Sync + 'static>(
        mut transport: T,
    ) -> ClientResult<T> {
        // Send empty message to notify connection
        transport
            .send(vec![0])
            .await
            .map_err(|_| ClientError::TransportError)?;

        match transport.receive().await {
            Ok(response) => {
                if let TransportEvent::Connect = response {
                    transport.establish_connection();
                    return Ok(transport);
                } else {
                    Err(ClientError::TransportError)
                }
            }
            Err(_) => Err(ClientError::TransportError),
        }
    }

    pub async fn create_port(&mut self, port_name: &str) -> ClientResult<&RpcClientPort> {
        let response = self
            .client_request_dispatcher
            .request::<CreatePortResponse, _, _>(|message_id| CreatePort {
                message_identifier: build_message_identifier(
                    RpcMessageTypes::RpcMessageTypes_CREATE_PORT as u32,
                    message_id,
                ),
                port_name: port_name.to_string(),
                ..Default::default()
            })
            .await?;

        let rpc_client_port = RpcClientPort::new(
            port_name,
            response.2.port_id,
            self.client_request_dispatcher.clone(),
        );

        self.ports.insert(port_name.to_string(), rpc_client_port);

        Ok(self.ports.get(port_name).unwrap())
    }
}

pub struct RpcClientPort {
    pub port_name: String,
    port_id: u32,
    client_request_dispatcher: Arc<ClientRequestDispatcher>,
}

impl RpcClientPort {
    fn new(name: &str, id: u32, dispatcher: Arc<ClientRequestDispatcher>) -> Self {
        Self {
            port_name: name.to_string(),
            port_id: id,
            client_request_dispatcher: dispatcher,
        }
    }

    pub async fn load_module(&self, module_name: &str) -> ClientResult<RpcClientModule> {
        let response: (u32, u32, RequestModuleResponse) = self
            .client_request_dispatcher
            .request(|message_id| RequestModule {
                port_id: self.port_id,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE as u32,
                    message_id,
                ),
                module_name: module_name.to_string(),
                ..Default::default()
            })
            .await
            .map_err(|err| err)?;

        let mut procedures = HashMap::new();

        for procedure in response.2.procedures {
            let (procedure_name, procedure_id) =
                (procedure.get_procedure_name(), procedure.get_procedure_id());

            procedures.insert(procedure_name.to_string(), procedure_id);
        }

        let client_module = RpcClientModule::new(
            module_name,
            response.2.port_id,
            procedures,
            self.client_request_dispatcher.clone(),
        );

        Ok(client_module)
    }
}

pub struct RpcClientModule {
    pub module_name: String,
    port_id: u32,
    procedures: HashMap<String, u32>,
    client_request_dispatcher: Arc<ClientRequestDispatcher>,
}

impl RpcClientModule {
    fn new(
        name: &str,
        port_id: u32,
        procedures: HashMap<String, u32>,
        dispatcher: Arc<ClientRequestDispatcher>,
    ) -> Self {
        Self {
            module_name: name.to_string(),
            port_id,
            procedures,
            client_request_dispatcher: dispatcher,
        }
    }

    pub async fn call_unary_procedure<ReturnType: Message, M: Message>(
        &self,
        procedure_name: &str,
        payload: M,
    ) -> ClientResult<ReturnType> {
        let procedure_id = self.procedures.get(procedure_name);
        if procedure_id.is_none() {
            return Err(ClientError::ProcedureNotFound);
        }
        let procedure_id = procedure_id.unwrap().to_owned();
        let payload = payload
            .write_to_bytes()
            .map_err(|_| ClientError::ProtocolError)?;
        let response: (u32, u32, Response) = self
            .client_request_dispatcher
            .request(|message_id| Request {
                port_id: self.port_id,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::RpcMessageTypes_REQUEST as u32,
                    message_id,
                ),
                procedure_id,
                payload,
                ..Default::default()
            })
            .await
            .map_err(|err| err)?;

        let returned_type = ReturnType::parse_from_bytes(&response.2.payload)
            .map_err(|_| ClientError::ProtocolError)?;

        Ok(returned_type)
    }
}

struct ClientRequestDispatcher {
    next_message_id: RwLock<u32>,
    transport: Arc<dyn Transport + Send + Sync>,
}

impl ClientRequestDispatcher {
    pub fn new(transport: Arc<dyn Transport + Send + Sync>) -> Self {
        Self {
            next_message_id: RwLock::new(1),
            transport,
        }
    }

    pub async fn request<ReturnType: Message, M: Message, Callback: FnOnce(u32) -> M>(
        &self,
        cb: Callback,
    ) -> ClientResult<(u32, u32, ReturnType)> {
        let message_id = {
            let message_id = self.next_message_id.read().await;
            *message_id
        };
        let payload = cb(message_id);

        let payload = payload
            .write_to_bytes()
            .map_err(|_| ClientError::ProtocolError)?;

        self.transport
            .send(payload)
            .await
            .map_err(|_| ClientError::TransportError)?;

        let response = self
            .transport
            .receive()
            .await
            .map_err(|_| ClientError::TransportError)?;

        if let TransportEvent::Message(data) = response {
            let mut next_message_id = self.next_message_id.write().await;
            *next_message_id += 1;

            match parse_protocol_message::<ReturnType>(&data) {
                Some(result) => Ok(result),
                None => Err(ClientError::ProtocolError),
            }
        } else {
            Err(ClientError::UnknownError)
        }
    }
}
