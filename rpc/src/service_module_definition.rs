//! It contains all the types related to the auto-codegeneration for a defined RPC service when a `.proto` is compiled.
//!
//! A [`ServiceModuleDefinition`] is auto-generated when a `.proto` is compiled. And it's filled with the defined procedures.
//! Actually, you probably don't need to use this module or their types.
//!
use crate::{rpc_protocol::RemoteError, stream_protocol::Generator};
use core::future::Future;
use std::{collections::HashMap, pin::Pin, sync::Arc};

/// The context type received by every procedure
pub struct ProcedureContext<Context> {
    /// The context given to the [`RpcServer`](`crate::server::RpcServer`)
    pub server_context: Arc<Context>,
    /// The ID given to the Transport which is calling the procedure
    pub transport_id: u32,
}

/// General type returned by every procedure
pub type Response<T> = Pin<Box<dyn Future<Output = Result<T, RemoteError>> + Send>>;

/// Payload for procedures which don't receive a stream
pub type CommonPayload = Vec<u8>;

/// Response type returned by a unary procedure
pub type UnaryResponse = Response<Vec<u8>>;

/// Handler type for a unary procedure.
pub type UnaryRequestHandler<Context> =
    dyn Fn(CommonPayload, ProcedureContext<Context>) -> UnaryResponse + Send + Sync;

/// Response type returned by a server streams procedure
pub type ServerStreamsResponse = Response<Generator<Vec<u8>>>;

/// Handler type for a server streams procedure
pub type ServerStreamsRequestHandler<Context> =
    dyn Fn(CommonPayload, ProcedureContext<Context>) -> ServerStreamsResponse + Send + Sync;

/// Payload type that a client streams procedure receives
pub type ClientStreamsPayload = Generator<Vec<u8>>;

/// Response type returned by a client streams procedure
pub type ClientStreamsResponse = Response<Vec<u8>>;

/// Handler type for a client streams procedure
pub type ClientStreamsRequestHandler<Context> =
    dyn Fn(ClientStreamsPayload, ProcedureContext<Context>) -> ClientStreamsResponse + Send + Sync;

/// Payload type that a bidirection streams procedure rceives
pub type BiStreamsPayload = Generator<Vec<u8>>;

/// Response type returned by a bidirectional streams procedure
pub type BiStreamsResponse = Response<Generator<Vec<u8>>>;

/// Handler type for a bidirectional streams procedure
pub type BiStreamsRequestHandler<Context> =
    dyn Fn(BiStreamsPayload, ProcedureContext<Context>) -> BiStreamsResponse + Send + Sync;

/// Type used for storing procedure definitions given by the codegeneration for the RPC service
pub enum ProcedureDefinition<Context> {
    /// Stores unary procedure definitions. Unary Procedure means a basic request<>response
    Unary(Arc<UnaryRequestHandler<Context>>),
    /// Stores server streams procedure definitions. [`crate::client::RpcClient`] sends a request and waits for the [`crate::server::RpcServer`] to send all the data that it has and close the stream opened
    ServerStreams(Arc<ServerStreamsRequestHandler<Context>>),
    /// Stores client strems procedure definitions. [`crate::client::RpcClient`] sends a request and opens a stream in the [`crate::server::RpcServer`], then [`crate::server::RpcServer`] waits for [`crate::client::RpcClient`] to send all the payloads
    ClientStreams(Arc<ClientStreamsRequestHandler<Context>>),
    /// Stores bidirectional streams procedure definitions. A stream is opened on both sides (client and server)
    BiStreams(Arc<BiStreamsRequestHandler<Context>>),
}

impl<Context> Clone for ProcedureDefinition<Context> {
    fn clone(&self) -> Self {
        match self {
            Self::Unary(procedure) => Self::Unary(procedure.clone()),
            Self::ServerStreams(procedure) => Self::ServerStreams(procedure.clone()),
            Self::ClientStreams(procedure) => Self::ClientStreams(procedure.clone()),
            Self::BiStreams(procedure) => Self::BiStreams(procedure.clone()),
        }
    }
}

/// It stores all procedures defined for a RPC service
pub struct ServiceModuleDefinition<Context> {
    /// Map that stores all procedures for the service
    ///
    /// the key is the procedure's name and the value is the handler for the procedure
    ///
    procedure_definitions: HashMap<String, ProcedureDefinition<Context>>,
}

impl<Context> ServiceModuleDefinition<Context> {
    pub fn new() -> Self {
        Self {
            procedure_definitions: HashMap::new(),
        }
    }

    /// Add an unary procedure handler to the service definition
    pub fn add_unary<
        H: Fn(CommonPayload, ProcedureContext<Context>) -> UnaryResponse + Send + Sync + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, ProcedureDefinition::Unary(Arc::new(handler)));
    }

    /// Add a server streams procedure handler to the service definition
    pub fn add_server_streams<
        H: Fn(CommonPayload, ProcedureContext<Context>) -> ServerStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, ProcedureDefinition::ServerStreams(Arc::new(handler)));
    }

    /// Add a client streams procedure handler to the service definition
    pub fn add_client_streams<
        H: Fn(ClientStreamsPayload, ProcedureContext<Context>) -> ClientStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, ProcedureDefinition::ClientStreams(Arc::new(handler)));
    }

    /// Add a bidirectional streams procedure handler to the service definition
    pub fn add_bidir_streams<
        H: Fn(BiStreamsPayload, ProcedureContext<Context>) -> BiStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, ProcedureDefinition::BiStreams(Arc::new(handler)));
    }

    fn add_definition(&mut self, name: &str, definition: ProcedureDefinition<Context>) {
        self.procedure_definitions
            .insert(name.to_string(), definition);
    }

    pub fn get_definitions(&self) -> &HashMap<String, ProcedureDefinition<Context>> {
        &self.procedure_definitions
    }
}

impl<Context> Default for ServiceModuleDefinition<Context> {
    fn default() -> Self {
        Self::new()
    }
}
