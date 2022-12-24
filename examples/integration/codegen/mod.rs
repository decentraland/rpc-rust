use std::sync::Arc;

use protobuf::Message;
use rpc_rust::{server::RpcServerPort, types::ServiceModuleDefinition};

use crate::{
    service::api::{Book, GetBookRequest},
    MyExampleContext,
};

pub const SERVICE: &str = "BookService";

#[async_trait::async_trait]
pub trait BookServiceInterface {
    async fn get_book(&self, request: GetBookRequest, context: Arc<MyExampleContext>) -> Book;
}

pub struct BookServiceCodeGen {}

impl BookServiceCodeGen {
    pub fn register_service<S: BookServiceInterface + Send + Sync + 'static>(
        port: &mut RpcServerPort<MyExampleContext>,
        service: S,
    ) {
        println!("> BookServiceCodeGen > register_service");
        let mut service_def = ServiceModuleDefinition::new();
        // Share service ownership
        let service = Arc::new(service);
        service_def.add_definition("GetBook".to_string(), move |request, context| {
            let req = Vec::from(request);
            let serv = service.clone();
            Box::pin(async move {
                let res = serv
                    .get_book(GetBookRequest::parse_from_bytes(&req).unwrap(), context)
                    .await;
                println!("> Service Definition > Get Book > response: {:?}", res);
                res.write_to_bytes().unwrap()
            })
        });
        port.register_module(SERVICE.to_string(), service_def)
    }
}
