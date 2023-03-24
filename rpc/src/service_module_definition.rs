//! It contains all the types related to the auto-codegeneration for a defined RPC service when a `.proto` is compiled.
//!
//! A [`ServiceModuleDefinition`] is auto-generated when a `.proto` is compiled. And it's filled with the defined procedures.
//! Actually, you probably don't need to use this module or their types.
//!
use core::future::Future;
use std::{collections::HashMap, pin::Pin, sync::Arc};

use crate::stream_protocol::Generator;

/// General type returned by every procedure
pub type Response<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Payload for procedures which don't receive a stream
pub type CommonPayload = Vec<u8>;

/// Response type returned by a unary procedure
pub type UnaryResponse = Response<Vec<u8>>;

/// Handler type for a unary procedure.
pub type UnaryRequestHandler<Context, Transport> =
    dyn Fn(CommonPayload, Arc<Context>, Arc<Transport>) -> UnaryResponse + Send + Sync;

/// Response type returned by a server streams procedure
pub type ServerStreamsResponse = Response<Generator<Vec<u8>>>;

/// Handler type for a server streams procedure
pub type ServerStreamsRequestHandler<Context, Transport> =
    dyn Fn(CommonPayload, Arc<Context>, Arc<Transport>) -> ServerStreamsResponse + Send + Sync;

/// Payload type that a client streams procedure receives
pub type ClientStreamsPayload = Generator<Vec<u8>>;

/// Response type returned by a client streams procedure
pub type ClientStreamsResponse = Response<Vec<u8>>;

/// Handler type for a client streams procedure
pub type ClientStreamsRequestHandler<Context, Transport> = dyn Fn(ClientStreamsPayload, Arc<Context>, Arc<Transport>) -> ClientStreamsResponse
    + Send
    + Sync;

/// Payload type that a bidirection streams procedure rceives
pub type BiStreamsPayload = Generator<Vec<u8>>;

/// Response type returned by a bidirectional streams procedure
pub type BiStreamsResponse = Response<Generator<Vec<u8>>>;

/// Handler type for a bidirectional streams procedure
pub type BiStreamsRequestHandler<Context, Transport> =
    dyn Fn(BiStreamsPayload, Arc<Context>, Arc<Transport>) -> BiStreamsResponse + Send + Sync;

/// Type used for storing procedure definitions given by the codegeneration for the RPC service
pub enum Definition<Context, Transport: ?Sized> {
    /// Store unary procedure definitions
    Unary(Arc<UnaryRequestHandler<Context, Transport>>),
    /// Store server streams procedure definitions
    ServerStreams(Arc<ServerStreamsRequestHandler<Context, Transport>>),
    /// Store client strems procedure definitions
    ClientStreams(Arc<ClientStreamsRequestHandler<Context, Transport>>),
    /// Store bidirectional streams procedure definitions
    BiStreams(Arc<BiStreamsRequestHandler<Context, Transport>>),
}

/// It stores all procedures defined for a RPC service
pub struct ServiceModuleDefinition<Context, Transport: ?Sized> {
    /// Map that stores all procedures for the service
    ///
    /// the key is the procedure's name and the value is the handler for the procedure
    ///
    definitions: HashMap<String, Definition<Context, Transport>>,
}

impl<Context, Transport: ?Sized> ServiceModuleDefinition<Context, Transport> {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    /// Add an unary procedure handler to the service definition
    pub fn add_unary<
        H: Fn(CommonPayload, Arc<Context>, Arc<Transport>) -> UnaryResponse + Send + Sync + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, Definition::Unary(Arc::new(handler)));
    }

    /// Add a server streams procedure handler to the service definition
    pub fn add_server_streams<
        H: Fn(CommonPayload, Arc<Context>, Arc<Transport>) -> ServerStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, Definition::ServerStreams(Arc::new(handler)));
    }

    /// Add a client streams procedure handler to the service definition
    pub fn add_client_streams<
        H: Fn(ClientStreamsPayload, Arc<Context>, Arc<Transport>) -> ClientStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, Definition::ClientStreams(Arc::new(handler)));
    }

    /// Add a bidirectional streams procedure handler to the service definition
    pub fn add_bidir_streams<
        H: Fn(BiStreamsPayload, Arc<Context>, Arc<Transport>) -> BiStreamsResponse
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, Definition::BiStreams(Arc::new(handler)));
    }

    fn add_definition(&mut self, name: &str, definition: Definition<Context, Transport>) {
        self.definitions.insert(name.to_string(), definition);
    }

    pub fn get_definitions(&self) -> &HashMap<String, Definition<Context, Transport>> {
        &self.definitions
    }
}

impl<Context, Transport> Default for ServiceModuleDefinition<Context, Transport> {
    fn default() -> Self {
        Self::new()
    }
}
