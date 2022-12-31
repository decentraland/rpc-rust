use rpc_rust::client::{RpcClientModule, ServiceClient};

use crate::service::api::{Book, GetBookRequest};

#[async_trait::async_trait]
pub trait BookServiceClientInterface {
    async fn get_book(&self, payload: GetBookRequest) -> Book;
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
}
