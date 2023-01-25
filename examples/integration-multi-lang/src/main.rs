use integration_multi_lang::{
    codegen::server::BookServiceCodeGen, service::book_service, Book, MyExampleContext,
};
use rpc_rust::{
    server::{RpcServer, RpcServerPort},
    transports::web_socket::WebSocketTransport,
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
    let server_handle = tokio::spawn(async { WebSocketTransport::listen("127.0.0.1:8080").await });

    let server_transport = server_handle
        .await
        .expect("Thread to finish")
        .expect("Server to accept client connection");

    println!("> RpcServer > Server transport is ready");

    let ctx = MyExampleContext {
        hardcoded_database: create_db(),
    };

    let mut server = RpcServer::create(ctx);

    server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
        BookServiceCodeGen::register_service(port, book_service::BookService {})
    });

    match server.attach_transport(server_transport) {
        Ok(_) => {
            println!("> RpcServer > transport attached successfully");
        }
        Err(_) => {
            println!("> RpcServer > unable to attach transport");
            panic!()
        }
    }

    server.run().await;
}
