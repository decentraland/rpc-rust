// THIS CODE SHOULD BE AUTO-GENERATED
use rpc_rust::{
    client::{RpcClientModule, ServiceClient},
    stream_protocol::Generator,
};

use crate::{Book, GetBookRequest, QueryBooksRequest};

use super::ClientStreamRequest;

#[async_trait::async_trait]
pub trait BookServiceClientInterface {
    async fn get_book(&self, payload: GetBookRequest) -> Book;
    async fn query_books(&self, payload: QueryBooksRequest) -> Generator<Book>;
    async fn get_book_stream(&self, payload: ClientStreamRequest<GetBookRequest>) -> Book;
}

pub struct BookServiceClient {
    rpc_client_module: RpcClientModule,
}

impl ServiceClient for BookServiceClient {
    fn set_client_module(rpc_client_module: RpcClientModule) -> Self {
        Self { rpc_client_module }
    }
}

#[async_trait::async_trait]
impl BookServiceClientInterface for BookServiceClient {
    async fn get_book(&self, payload: GetBookRequest) -> Book {
        self.rpc_client_module
            .call_unary_procedure("GetBook", payload)
            .await
            .unwrap()
    }
    async fn query_books(&self, payload: QueryBooksRequest) -> Generator<Book> {
        self.rpc_client_module
            .call_server_streams_procedure("QueryBooks", payload)
            .await
            .unwrap()
    }
    async fn get_book_stream(&self, payload: ClientStreamRequest<GetBookRequest>) -> Book {
        self.rpc_client_module
            .call_client_streams_procedure("GetBookStream", payload)
            .await
            .unwrap()
    }
}
