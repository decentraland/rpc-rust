use std::{collections::HashMap, sync::Arc};

use log::debug;
use prost::Message;
use tokio::sync::{oneshot, Mutex};

use crate::{
    messages_handlers::ClientMessagesHandler,
    protocol::{
        parse::{build_message_identifier, parse_protocol_message},
        CreatePort, CreatePortResponse, Request, RequestModule, RequestModuleResponse, Response,
        RpcMessageTypes,
    },
    stream_protocol::{Stream, StreamProtocol},
    transports::{Transport, TransportEvent},
};

pub trait ServiceClient {
    fn set_client_module(client_module: RpcClientModule) -> Self;
}

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
        let transport = Self::establish_connection(transport).await?;

        let client_request_dispatcher = Arc::new(ClientRequestDispatcher::new(transport));
        client_request_dispatcher.start();

        Ok(Self::from_dispatcher(client_request_dispatcher))
    }

    fn from_dispatcher(client_request_dispatcher: Arc<ClientRequestDispatcher>) -> Self {
        Self {
            ports: HashMap::new(),
            client_request_dispatcher,
        }
    }

    async fn establish_connection<T: Transport + Send + Sync + 'static>(
        transport: T,
    ) -> ClientResult<T> {
        // Send empty message to notify connection
        transport
            .send(vec![0])
            .await
            .map_err(|_| ClientError::TransportError)?;

        match transport.receive().await {
            Ok(TransportEvent::Connect) => Ok(transport),
            _ => Err(ClientError::TransportError),
        }
    }

    pub async fn create_port(&mut self, port_name: &str) -> ClientResult<&RpcClientPort> {
        let response = self
            .client_request_dispatcher
            .request::<CreatePortResponse, _, _>(|message_id| CreatePort {
                message_identifier: build_message_identifier(
                    RpcMessageTypes::CreatePort as u32,
                    message_id,
                ),
                port_name: port_name.to_string(),
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

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.client_request_dispatcher.stop();
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

    pub async fn load_module<S: ServiceClient>(&self, module_name: &str) -> ClientResult<S> {
        let response: (u32, u32, RequestModuleResponse) = self
            .client_request_dispatcher
            .request(|message_id| RequestModule {
                port_id: self.port_id,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::RequestModule as u32,
                    message_id,
                ),
                module_name: module_name.to_string(),
            })
            .await?;

        let mut procedures = HashMap::new();

        for procedure in response.2.procedures {
            let (procedure_name, procedure_id) = (procedure.procedure_name, procedure.procedure_id);

            procedures.insert(procedure_name.to_string(), procedure_id);
        }

        let client_module = RpcClientModule::new(
            module_name,
            response.2.port_id,
            procedures,
            self.client_request_dispatcher.clone(),
        );

        let client_service = S::set_client_module(client_module);

        Ok(client_service)
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

    pub async fn call_unary_procedure<ReturnType: Message + Default, M: Message + Default>(
        &self,
        procedure_name: &str,
        payload: M,
    ) -> ClientResult<ReturnType> {
        let response: (u32, u32, Response) = self.call_procedure(procedure_name, payload).await?;

        let returned_type = ReturnType::decode(response.2.payload.as_slice())
            .map_err(|_| ClientError::ProtocolError)?;

        Ok(returned_type)
    }

    pub async fn call_server_streams_procedure<
        M: Message,
        ReturnType: Message + Default + 'static,
    >(
        &self,
        procedure_name: &str,
        payload: M,
    ) -> ClientResult<Stream<ReturnType>> {
        let response: (u32, u32, Response) = self.call_procedure(procedure_name, payload).await?;
        let stream_protocol = self
            .client_request_dispatcher
            .stream_server_messages(self.port_id, response.1)
            .await;

        if let Err(_) = stream_protocol.acknowledge_open().await {
            return Err(ClientError::TransportError);
        }

        Ok(stream_protocol)
    }

    async fn call_procedure<M: Message, ReturnType: Message + Default>(
        &self,
        procedure_name: &str,
        payload: M,
    ) -> ClientResult<(u32, u32, ReturnType)> {
        let procedure_id = self.procedures.get(procedure_name);
        if procedure_id.is_none() {
            return Err(ClientError::ProcedureNotFound);
        }
        let procedure_id = procedure_id.unwrap().to_owned();
        let payload = payload.encode_to_vec();
        let response: (u32, u32, ReturnType) = self
            .client_request_dispatcher
            .request(|message_id| Request {
                port_id: self.port_id,
                message_identifier: build_message_identifier(
                    RpcMessageTypes::Request as u32,
                    message_id,
                ),
                procedure_id,
                payload,
            })
            .await?;

        Ok(response)
    }
}

struct ClientRequestDispatcher {
    next_message_id: Mutex<u32>,
    client_messages_handler: Arc<ClientMessagesHandler>,
}

impl ClientRequestDispatcher {
    pub fn new<T: Transport + Send + Sync + 'static>(transport: T) -> Self {
        Self {
            next_message_id: Mutex::new(1),
            client_messages_handler: Arc::new(ClientMessagesHandler::new(Arc::new(transport))),
        }
    }

    fn start(&self) {
        self.client_messages_handler.clone().start();
    }

    fn stop(&self) {
        self.client_messages_handler.stop()
    }

    pub async fn request<
        ReturnType: Message + Default,
        M: Message + Default,
        Callback: FnOnce(u32) -> M,
    >(
        &self,
        cb: Callback,
    ) -> ClientResult<(u32, u32, ReturnType)> {
        let (payload, current_request_message_id) = {
            let mut message_lock = self.next_message_id.lock().await;
            let message_id = *message_lock;
            debug!("Message ID: {}", message_id);
            let payload = cb(message_id);
            // store next_message_id
            *message_lock += 1;
            (payload, message_id)
        }; // Force to drop the mutex for other conccurrent operations

        let payload = payload.encode_to_vec();
        self.client_messages_handler
            .transport
            .send(payload)
            .await
            .map_err(|_| ClientError::TransportError)?;

        let (tx, rx) = oneshot::channel::<Vec<u8>>();

        self.client_messages_handler
            .register_one_time_listener(current_request_message_id, tx)
            .await;

        let response = rx.await.map_err(|_| ClientError::TransportError)?;

        match parse_protocol_message::<ReturnType>(&response) {
            Some(result) => Ok(result),
            None => Err(ClientError::ProtocolError),
        }
    }

    async fn stream_server_messages<M: Message + Default + 'static>(
        &self,
        port_id: u32,
        message_id: u32,
    ) -> Stream<M> {
        let (stream_protocol, listener) = StreamProtocol::create(
            self.client_messages_handler.transport.clone(),
            port_id,
            message_id,
        );

        self.client_messages_handler
            .register_listener(message_id, listener)
            .await;

        let client_messages_handler_listener_removal = self.client_messages_handler.clone();
        stream_protocol.start_processing(move || async move {
            // Callback for remove listener
            client_messages_handler_listener_removal
                .unregister_listener(message_id)
                .await;
        });

        Stream(stream_protocol)
    }
}
