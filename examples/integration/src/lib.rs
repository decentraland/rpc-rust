use tokio::sync::RwLock;

pub mod service;
include!(concat!(env!("OUT_DIR"), "/_.rs"));

pub struct MyExampleContext {
    pub hardcoded_database: RwLock<Vec<Book>>,
}
