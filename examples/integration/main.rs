extern crate protoc_rust;

mod codegen;
mod service;

use protobuf::Message;
use rpc_rust::{
    protocol::{
        index::{
            CreatePort, CreatePortResponse, Request, RequestModule, RequestModuleResponse, Response,
        },
        parse::build_message_identifier,
    },
    server::{RpcServer, RpcServerPort},
    transports::{self, Transport, TransportEvent},
};

use service::{api, book_service};

use crate::service::api::GetBookRequest;

pub struct BookRecord {
    pub author: String,
    pub title: String,
    pub isbn: i64,
}

pub struct MyExampleContext {
    pub hardcoded_database: Vec<BookRecord>,
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

    async_scoped::TokioScope::scope_and_block(|scope| {
        // 1- Create Transport
        let (mut client_transport, server_transport) =
            transports::memory::MemoryTransport::create();

        scope.spawn(async move {
            let connect_message = vec![0];
            client_transport.send(connect_message).await.unwrap();

            let create_port = CreatePort {
                message_identifier: build_message_identifier(5, 1),
                port_name: "testport".to_string(),
                ..Default::default()
            };

            let _result = client_transport
                .send(create_port.write_to_bytes().unwrap())
                .await;

            // Wait for the server response
            let res = client_transport.receive().await.unwrap();
            println!("> Create Port > Server Response Type: {:?}", res);
            match res {
                TransportEvent::Message(bytes) => {
                    let port_response = CreatePortResponse::parse_from_bytes(&bytes).unwrap();
                    println!("> Create Port > Server Response body: {:?}", port_response)
                }
                _ => {
                    println!("> Create Port > Server Response not message type")
                }
            }

            let request_module = RequestModule {
                port_id: 1,
                message_identifier: build_message_identifier(7, 2),
                module_name: "BookService".to_string(),
                ..Default::default()
            };

            let _result = client_transport
                .send(request_module.write_to_bytes().unwrap())
                .await;

            let res = client_transport.receive().await.unwrap();

            println!("> Request Module > Server Response Type: {:?}", res);
            match res {
                TransportEvent::Message(bytes) => {
                    let request_module_response =
                        RequestModuleResponse::parse_from_bytes(&bytes).unwrap();
                    println!(
                        "> Request Module > Server Response body: {:?}",
                        request_module_response
                    )
                }
                _ => {
                    println!("> Request Module > Server Response not message type")
                }
            }

            let mut get_book_payload = GetBookRequest::default();
            get_book_payload.set_isbn(1000);

            let call_procedure = Request {
                port_id: 1,
                message_identifier: build_message_identifier(1, 3),
                procedure_id: 1,
                payload: get_book_payload.write_to_bytes().unwrap(),
                ..Default::default()
            };

            let _result = client_transport
                .send(call_procedure.write_to_bytes().unwrap())
                .await;

            let res = client_transport.receive().await.unwrap();

            println!("> Call Procedure > Server Response Type: {:?}", res);
            match res {
                TransportEvent::Message(bytes) => {
                    let procedure_response = Response::parse_from_bytes(&bytes).unwrap();
                    let payload = api::Book::parse_from_bytes(&procedure_response.payload).unwrap();
                    println!(
                        "> Call Procedure > Server Response body: {:?}",
                        procedure_response
                    );
                    println!(
                        "> Call Procedure > Server Response body > payload: {:?}",
                        payload
                    )
                }
                _ => {
                    println!("> Call Procedure > Server Response not message type")
                }
            }
        });

        scope.spawn(async {
            let ctx = MyExampleContext {
                hardcoded_database: vec![
                    BookRecord {
                        author: "mr steve".to_string(),
                        title: "Rust: crash course".to_string(),
                        isbn: 1000,
                    },
                    BookRecord {
                        author: "mr steve".to_string(),
                        title: "Rust: how do futures work under the hood?".to_string(),
                        isbn: 1001,
                    },
                ],
            };

            // 2- Create Server with Transport
            let mut server = RpcServer::create(ctx);
            // 3- Server listen to Create Port request
            server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
                println!("Port {} created!", port.name);
                codegen::BookServiceCodeGen::register_service(port, book_service::BookService {})
            });

            server.attach_transport(server_transport);

            server.run().await;
        });
    });
}
