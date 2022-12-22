use std::{collections::HashMap, sync::Arc};

pub type UnaryRequestHandler<CTX> = dyn Fn(&[u8], &CTX) -> Vec<u8> + Send + Sync;

#[derive(Default)]
pub struct ServiceModuleDefinition<CTX> {
    definitions: HashMap<String, Arc<Box<UnaryRequestHandler<CTX>>>>,
}

impl<CTX> ServiceModuleDefinition<CTX> {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    pub fn add_definition<H: Fn(&[u8], &CTX) -> Vec<u8> + Send + Sync + 'static>(
        &mut self,
        name: String,
        handler: H,
    ) {
        self.definitions.insert(name, Arc::new(Box::new(handler)));
    }

    pub fn get_definitions(&self) -> &HashMap<String, Arc<Box<UnaryRequestHandler<CTX>>>> {
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
