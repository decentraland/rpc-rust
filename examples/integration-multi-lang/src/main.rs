use std::sync::Arc;

use dcl_rpc::{
    server::{RpcServer, RpcServerPort},
    transports::web_sockets::{
        tungstenite::{TungsteniteWebSocket, WebSocketServer},
        WebSocketTransport,
    },
};
use integration_multi_lang::{
    service::book_service, Book, BookServiceRegistration, MyExampleContext,
};

fn create_db() -> Vec<Book> {
    let book_1 = Book {
        author: "mr steve".to_string(),
        title: "Rust: crash course".to_string(),
        isbn: 1000,
    };

    let book_2 = Book {
        author: "mr jobs".to_string(),
        title: "Rust: how do futures work under the hood?".to_string(),
        isbn: 1001,
    };

    let book_3 = Book {
        author: "mr robot".to_string(),
        title: "Create a robot from scrath".to_string(),
        isbn: 1002,
    };

    let book_4 = Book {
        author: "vitalik".to_string(),
        title: "Blockchain 101".to_string(),
        isbn: 1003,
    };

    let book_5 = Book {
        author: "buterin".to_string(),
        title: "Smart Contracts 101".to_string(),
        isbn: 1004,
    };
    vec![book_1, book_2, book_3, book_4, book_5]
}

#[tokio::main]
async fn main() {
    println!("--- Running multi-lang example with Web Socket Transports ---");
    run_ws_example().await
}

async fn run_ws_example() {
    let mut ws_server = WebSocketServer::new("127.0.0.1:8080");

    // Listen in background task
    let mut connection_listener = ws_server.listen().await.unwrap();

    println!("> RpcServer > Server transport is ready");

    let ctx = MyExampleContext {
        hardcoded_database: create_db(),
    };

    let mut server = RpcServer::create(ctx);
    server.set_module_registrator_handler(|port: &mut RpcServerPort<MyExampleContext>| {
        BookServiceRegistration::register_service(port, book_service::BookService {});
    });

    let server_events_sender = server.get_server_events_sender();
    tokio::spawn(async move {
        while let Some(Ok(connection)) = connection_listener.recv().await {
            let websocket = Arc::new(TungsteniteWebSocket::new(connection));
            let transport = WebSocketTransport::new(websocket);
            let transport_to_arc = Arc::new(transport);
            match server_events_sender.send_attach_transport(transport_to_arc) {
                Ok(_) => {
                    println!("> RpcServer > transport attached successfully");
                }
                Err(_) => {
                    println!("> RpcServer > unable to attach transport");
                    panic!()
                }
            }
        }
    });

    // PARTICULAR CASE IN ORDER TO EXIT THE PROCESS WHEN THE EXAMPLE CLIENT CLOSES THE CONNECTION
    server.set_on_transport_closes_handler(|_, _| {
        println!("> Exit process..");
        std::process::exit(0);
    });

    server.run().await;
}
