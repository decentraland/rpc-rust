pub mod service;
include!(concat!(env!("OUT_DIR"), "/_.rs"));

pub struct MyExampleContext {
    pub hardcoded_database: Vec<Book>,
}
pub struct WSTransportContext {
    pub connection_id: usize,
}
