extern crate protoc_rust;

mod codegen;
mod service;

use rpc_rust::{
    client::RpcClient,
    server::{RpcServer, RpcServerPort},
    transports,
};

use service::{api::Book, book_service};

use crate::service::api::GetBookRequest;

pub struct MyExampleContext {
    pub hardcoded_database: Vec<Book>,
}

fn create_db() -> Vec<Book> {
    let mut book_1 = Book::default();
    book_1.set_author("mr steve".to_string());
    book_1.set_title("Rust: crash course".to_string());
    book_1.set_isbn(1000);

    let mut book_2 = Book::default();
    book_2.set_author("mr jobs".to_string());
    book_2.set_title("Rust: how do futures work under the hood?".to_string());
    book_2.set_isbn(1001);

    vec![book_1, book_2]
}

#[tokio::main]
async fn main() {
    // Rebuild proto when run it
    protoc_rust::Codegen::new()
        .out_dir("examples/integration/service")
        .inputs(["examples/integration/service/api.proto"])
        .include("examples/integration/service")
        .run()
        .expect("Running protoc failed.");

    // 1- Create Transport
    let (client_transport, server_transport) = transports::memory::MemoryTransport::create();

    let client_handle = tokio::spawn(async move {
        let mut client = RpcClient::new(client_transport).await.unwrap();

        let client_port = match client.create_port("TEST_PORT").await {
            Ok(port) => {
                println!(
                    "> Create Port > Created successfully > Port name: {}",
                    port.port_name
                );
                port
            }
            Err(err) => {
                println!("> Create Port > Error on creating the port: {:?}", err);
                panic!()
            }
        };

        assert_eq!(client_port.port_name, "TEST_PORT");

        let book_service_module = client_port.load_module("BookService").await.unwrap();

        let mut get_book_payload = GetBookRequest::default();
        get_book_payload.set_isbn(1000);

        let response = book_service_module
            .call_unary_procedure::<Book, _>("GetBook", get_book_payload)
            .await
            .unwrap();

        println!("> Got Book: {:?}", response);

        assert_eq!(response.get_isbn(), 1000);
        assert_eq!(response.get_title(), "Rust: crash course");
        assert_eq!(response.get_author(), "mr steve");
    });

    let server_handle = tokio::spawn(async {
        let ctx = MyExampleContext {
            hardcoded_database: create_db(),
        };

        // 2- Create Server with Transport
        let mut server = RpcServer::create(ctx);
        // 3- Server listen to Create Port request
        server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            codegen::BookServiceCodeGen::register_service(port, book_service::BookService {})
        });

        server.attach_transport(server_transport);

        server.run().await;
    });

    client_handle.await.unwrap();
    server_handle.await.unwrap();
}
