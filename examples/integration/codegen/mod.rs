use protobuf::Message;
use rpc_rust::{server::RpcServerPort, types::ServiceModuleDefinition};

use crate::service::api::{Book, GetBookRequest};

pub const SERVICE: &str = "BookService";

pub trait BookServiceInterface {
    fn get_book(&self, request: GetBookRequest) -> Book;
}

pub struct BookServiceCodeGen {}

impl BookServiceCodeGen {
    pub fn register_service<S: BookServiceInterface + Send + Sync + 'static>(
        port: &mut RpcServerPort,
        service: S,
    ) {
        println!("> BookServiceCodeGen > register_service");
        let mut service_def = ServiceModuleDefinition::new();
        service_def.add_definition("GetBook".to_string(), move |request| {
            let res = service.get_book(GetBookRequest::parse_from_bytes(&request).unwrap());
            println!("> Service Definition > Get Book > response: {:?}", res);
            res.write_to_bytes().unwrap()
        });
        port.register_module(SERVICE.to_string(), service_def)
    }
}
