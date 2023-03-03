pub mod service;
pub mod setup_quic;
include!(concat!(env!("OUT_DIR"), "/_.rs"));

pub struct MyExampleContext {
    pub hardcoded_database: Vec<Book>,
}
