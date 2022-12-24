use core::future::Future;
use std::{collections::HashMap, pin::Pin, sync::Arc};

pub type UnaryRequestHandler<Context> =
    dyn Fn(Vec<u8>, Arc<Context>) -> Pin<Box<dyn Future<Output = Vec<u8>> + Send>> + Send + Sync;

#[derive(Default)]
pub struct ServiceModuleDefinition<Context> {
    definitions: HashMap<String, Arc<UnaryRequestHandler<Context>>>,
}

impl<Context> ServiceModuleDefinition<Context> {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    pub fn add_definition<
        H: Fn(Vec<u8>, Arc<Context>) -> Pin<Box<dyn Future<Output = Vec<u8>> + Send>>
            + Send
            + Sync
            + 'static,
    >(
        &mut self,
        name: String,
        handler: H,
    ) {
        self.definitions.insert(name, Arc::new(handler));
    }

    pub fn get_definitions(&self) -> &HashMap<String, Arc<UnaryRequestHandler<Context>>> {
        &self.definitions
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
