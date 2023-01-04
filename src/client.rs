use std::{collections::HashMap, sync::Arc};

use protobuf::Message;
use tokio::{
    select,
    sync::{
        oneshot::{self, Sender},
        Mutex,
    },
};
use tokio_util::sync::CancellationToken;

use crate::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, Request, RequestModule, RequestModuleResponse,
            Response, RpcMessageTypes,
        },
        parse::{build_message_identifier, parse_header, parse_protocol_message},
    },
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
        let transport = Box::new(transport);
        let client_request_dispatcher = Arc::new(ClientRequestDispatcher::new(transport));

        client_request_dispatcher.clone().start();

        let client = Self {
            ports: HashMap::new(),
            client_request_dispatcher,
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
            Ok(TransportEvent::Connect) => {
                transport.establish_connection();
                Ok(transport)
            }
            _ => Err(ClientError::TransportError),
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
                    RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE as u32,
                    message_id,
                ),
                module_name: module_name.to_string(),
                ..Default::default()
            })
            .await?;

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
            .await?;

        let returned_type = ReturnType::parse_from_bytes(&response.2.payload)
            .map_err(|_| ClientError::ProtocolError)?;

        Ok(returned_type)
    }
}

struct ClientRequestDispatcher {
    next_message_id: Mutex<u32>,
    transport: Box<dyn Transport + Send + Sync>,
    one_time_listeners: Mutex<HashMap<u32, oneshot::Sender<Vec<u8>>>>,
    process_cancellation_token: CancellationToken,
}

impl ClientRequestDispatcher {
    pub fn new(transport: Box<dyn Transport + Send + Sync>) -> Self {
        Self {
            next_message_id: Mutex::new(1),
            transport,
            one_time_listeners: Mutex::new(HashMap::new()),
            process_cancellation_token: CancellationToken::new(),
        }
    }

    fn start(self: Arc<Self>) {
        let this = self.clone();
        let token = self.process_cancellation_token.clone();
        tokio::spawn(async move {
            select! {
                _ = token.cancelled() => {
                    println!("> ClientRequestDispatcher cancelled!")
                },
                _ = this.process() => {

                }
            }
        });
    }

    fn stop(&self) {
        self.process_cancellation_token.cancel();
    }

    async fn process(&self) {
        loop {
            match self.transport.receive().await {
                Ok(TransportEvent::Message(data)) => {
                    let message_header = parse_header(&data);
                    // TODO: find a way to communicate the error of parsing the message_header
                    match message_header {
                        Some(message_header) => {
                            let mut read_callbacks = self.one_time_listeners.lock().await;
                            // We remove the listener in order to get a owned value and also remove it from memory
                            let sender = read_callbacks.remove(&message_header.1);
                            match sender.map(|sender| sender.send(data)) {
                                Some(Ok(_)) => {}
                                Some(Err(_)) => {
                                    println!(
                                        "> Client > Error on sending message through transport"
                                    );
                                    continue;
                                }
                                None => {
                                    println!(
                                        "> Client > No callback registered for message {} response",
                                        message_header.1
                                    );
                                    continue;
                                }
                            }
                        }
                        None => {
                            println!("> Client > Error on parsing message header");
                            continue;
                        }
                    }
                }
                Ok(_) => {
                    // Ignore another type of TransportEvent
                    continue;
                }
                Err(_) => {
                    println!("Client error on receiving");
                    break;
                }
            }
        }
    }

    pub async fn request<ReturnType: Message, M: Message, Callback: FnOnce(u32) -> M>(
        &self,
        cb: Callback,
    ) -> ClientResult<(u32, u32, ReturnType)> {
        let (payload, current_request_message_id) = {
            let mut message_lock = self.next_message_id.lock().await;
            let message_id = *message_lock;
            println!("Message ID: {}", message_id);
            let payload = cb(message_id);
            // store next_message_id
            *message_lock += 1;
            (payload, message_id)
        }; // Force to drop the mutex for other conccurrent operations

        let payload = payload
            .write_to_bytes()
            .map_err(|_| ClientError::ProtocolError)?;

        self.transport
            .send(payload)
            .await
            .map_err(|_| ClientError::TransportError)?;

        let (tx, rx) = oneshot::channel::<Vec<u8>>();

        self.register_one_time_listener(current_request_message_id, tx)
            .await;

        let response = rx.await.map_err(|_| ClientError::TransportError)?;

        match parse_protocol_message::<ReturnType>(&response) {
            Some(result) => Ok(result),
            None => Err(ClientError::ProtocolError),
        }
    }

    async fn register_one_time_listener(&self, message_id: u32, callback: Sender<Vec<u8>>) {
        let mut lock = self.one_time_listeners.lock().await;
        lock.insert(message_id, callback);
    }
}
