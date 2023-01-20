// THIS CODE SHOULD BE AUTO-GENERATED
use std::sync::Arc;

use prost::Message;
use rpc_rust::{server::RpcServerPort, types::ServiceModuleDefinition};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::{Book, GetBookRequest, QueryBooksRequest};

use super::ServerStreamResponse;

pub const SERVICE: &str = "BookService";

#[async_trait::async_trait]
pub trait BookServiceInterface<Context> {
    async fn get_book(&self, request: GetBookRequest, context: Arc<Context>) -> Book;
    async fn query_books(
        &self,
        request: QueryBooksRequest,
        context: Arc<Context>,
    ) -> ServerStreamResponse<Book>;
}

pub struct BookServiceCodeGen {}

impl BookServiceCodeGen {
    pub fn register_service<
        S: BookServiceInterface<Context> + Send + Sync + 'static,
        Context: Send + Sync + 'static,
    >(
        port: &mut RpcServerPort<Context>,
        service: S,
    ) {
        println!("> BookServiceCodeGen > register_service");
        let mut service_def = ServiceModuleDefinition::new();
        // Share service ownership
        let shareable_service = Arc::new(service);
        // Clone it for "GetBook" procedure
        let service = Arc::clone(&shareable_service);
        service_def.add_unary("GetBook", move |request, context| {
            let service = service.clone();
            Box::pin(async move {
                let response = service
                    .get_book(GetBookRequest::decode(request.as_slice()).unwrap(), context)
                    .await;
                println!("> Service Definition > Get Book > response: {:?}", response);
                response.encode_to_vec()
            })
        });

        let service = Arc::clone(&shareable_service);
        service_def.add_server_streams("QueryBooks", move |request, context| {
            let service = service.clone();
            Box::pin(async move {
                let (tx, rx): (UnboundedSender<Vec<u8>>, UnboundedReceiver<Vec<u8>>) =
                    unbounded_channel();
                let mut server_stream = service
                    .query_books(
                        QueryBooksRequest::decode(request.as_slice()).unwrap(),
                        context,
                    )
                    .await;

                tokio::spawn(async move {
                    while let Some(book) = server_stream.recv().await {
                        tx.send(book.encode_to_vec()).unwrap();
                    }
                });

                rx
            })
        });
        port.register_module(SERVICE.to_string(), service_def)
    }
}
