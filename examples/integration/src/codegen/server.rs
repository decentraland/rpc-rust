// THIS CODE SHOULD BE AUTO-GENERATED
use std::sync::Arc;

use dcl_rpc::{
    server::RpcServerPort, service_module_definition::ServiceModuleDefinition,
    stream_protocol::Generator,
};
use prost::Message;

use crate::{Book, GetBookRequest, QueryBooksRequest};

use super::{ClientStreamRequest, ServerStreamResponse};

pub const SERVICE: &str = "BookService";

#[async_trait::async_trait]
pub trait BookServiceInterface<Context> {
    async fn get_book(&self, request: GetBookRequest, context: Arc<Context>) -> Book;
    async fn query_books(
        &self,
        request: QueryBooksRequest,
        context: Arc<Context>,
    ) -> ServerStreamResponse<Book>;
    async fn get_book_stream(
        &self,
        request: ClientStreamRequest<GetBookRequest>,
        context: Arc<Context>,
    ) -> Book;
    async fn query_books_streams(
        &self,
        request: ClientStreamRequest<GetBookRequest>,
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
                let server_streams = service
                    .query_books(
                        QueryBooksRequest::decode(request.as_slice()).unwrap(),
                        context,
                    )
                    .await;

                // Transforming and filling the new generator is spawned so the response it quick
                Generator::from_generator(server_streams, |item| item.encode_to_vec())
            })
        });

        let service = Arc::clone(&shareable_service);
        service_def.add_client_streams("GetBookStream", move |request, context| {
            let serv = service.clone();
            Box::pin(async move {
                let generator = Generator::from_generator(request, |item| {
                    GetBookRequest::decode(item.as_slice()).unwrap()
                });

                let response = serv.get_book_stream(generator, context).await;
                response.encode_to_vec()
            })
        });

        let service = Arc::clone(&shareable_service);
        service_def.add_bidir_streams("QueryBooksStream", move |request, context| {
            let serv = service.clone();
            Box::pin(async move {
                let generator = Generator::from_generator(request, |item| {
                    GetBookRequest::decode(item.as_slice()).unwrap()
                });

                let response = serv.query_books_streams(generator, context).await;
                Generator::from_generator(response, |book| book.encode_to_vec())
            })
        });

        port.register_module(SERVICE.to_string(), service_def)
    }
}
