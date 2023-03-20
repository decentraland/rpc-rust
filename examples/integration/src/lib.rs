use tokio::sync::RwLock;

pub mod service;
pub mod setup_quic;
include!(concat!(env!("OUT_DIR"), "/_.rs"));

pub struct BookContext {
    pub hardcoded_database: Vec<Book>,
}

pub type MyExampleContext = RwLock<BookContext>;
