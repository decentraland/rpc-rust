use core::future::Future;
use std::{collections::HashMap, pin::Pin, sync::Arc};

use crate::stream_protocol::Generator;

pub type Response<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub type CommonPayload = Vec<u8>;

pub type UnaryResponse = Response<Vec<u8>>;

pub type UnaryRequestHandler<Context> =
    dyn Fn(CommonPayload, Arc<Context>) -> UnaryResponse + Send + Sync;

pub type ServerStreamsResponse = Response<Generator<Vec<u8>>>;

pub type ServerStreamsRequestHandler<Context> =
    dyn Fn(CommonPayload, Arc<Context>) -> ServerStreamsResponse + Send + Sync;

pub type ClientStreamsPayload = Generator<Vec<u8>>;

pub type ClientStreamsResponse = Response<Vec<u8>>;

pub type ClientStreamsRequestHandler<Context> =
    dyn Fn(ClientStreamsPayload, Arc<Context>) -> ClientStreamsResponse + Send + Sync;

pub enum Definition<Context> {
    Unary(Arc<UnaryRequestHandler<Context>>),
    ServerStreams(Arc<ServerStreamsRequestHandler<Context>>),
    ClientStreams(Arc<ClientStreamsRequestHandler<Context>>),
}

pub struct ServiceModuleDefinition<Context> {
    definitions: HashMap<String, Definition<Context>>,
}

impl<Context> ServiceModuleDefinition<Context> {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    pub fn add_unary<
        H: Fn(Vec<u8>, Arc<Context>) -> Pin<Box<dyn Future<Output = Vec<u8>> + Send>>
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: &str,
        handler: H,
    ) {
        self.add_definition(name, Definition::Unary(Arc::new(handler)));
    }

    pub fn add_server_streams<
        H: Fn(Vec<u8>, Arc<Context>) -> Pin<Box<dyn Future<Output = Generator<Vec<u8>>> + Send>>
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

    pub fn add_client_streams<
        H: Fn(Generator<Vec<u8>>, Arc<Context>) -> Pin<Box<dyn Future<Output = Vec<u8>> + Send>>
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

    fn add_definition(&mut self, name: &str, definition: Definition<Context>) {
        self.definitions.insert(name.to_string(), definition);
    }

    pub fn get_definitions(&self) -> &HashMap<String, Definition<Context>> {
        &self.definitions
    }
}

impl<Context> Default for ServiceModuleDefinition<Context> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct ServerModuleProcedures {
    pub procedure_name: String,
    pub procedure_id: u32,
}

pub struct ServerModuleDeclaration {
    pub procedures: Vec<ServerModuleProcedures>,
}
