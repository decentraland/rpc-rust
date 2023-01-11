use integration::{
    codegen::{
        client::{BookServiceClient, BookServiceClientInterface},
        server::BookServiceCodeGen,
    },
    service::book_service,
    setup_quic::{configure_client, generate_self_signed_cert},
    Book, GetBookRequest, MyExampleContext,
};
use quinn::ServerConfig;
use rpc_rust::{
    client::RpcClient,
    server::{RpcServer, RpcServerPort},
    transports::{
        self, memory::MemoryTransport, quic::QuicTransport, web_socket::WebSocketTransport,
        Transport,
    },
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
        isbn: 1000,
    };
    vec![book_1, book_2]
}

fn create_memory_transports() -> (MemoryTransport, MemoryTransport) {
    transports::memory::MemoryTransport::create()
}

async fn create_web_socket_transports() -> (WebSocketTransport, WebSocketTransport) {
    let server_handle = tokio::spawn(async { WebSocketTransport::listen("127.0.0.1:8080").await });
    let client_handle =
        tokio::spawn(async { WebSocketTransport::connect("ws://127.0.0.1:8080").await });

    let server = server_handle
        .await
        .expect("Thread to finish")
        .expect("Server to accept client connection");

    let client = client_handle
        .await
        .expect("Thread to finish")
        .expect("Client to establish connection");
    (client, server)
}

async fn create_quic_transports() -> (QuicTransport, QuicTransport) {
    let server_handle = tokio::spawn(async {
        let (cert, keys) = generate_self_signed_cert();
        QuicTransport::create_server(
            "0.0.0.0:8080",
            ServerConfig::with_single_cert(vec![cert], keys).expect("Can create server config"),
        )
        .await
    });
    let client_handle = tokio::spawn(async {
        let client_config = configure_client();
        QuicTransport::create_client("127.0.0.1:0", "127.0.0.1:8080", "localhost", client_config)
            .await
    });

    let server = server_handle
        .await
        .expect("Thread to finish")
        .expect("Server to accept client connection");

    let client = client_handle
        .await
        .expect("Thread to finish")
        .expect("Client to establish connection");
    (client, server)
}

#[tokio::main]
async fn main() {
    println!("--- Running example with Memory Transports ---");
    run_with_transports(create_memory_transports()).await;

    println!("--- Running example with Web Socket Transports ---");
    run_with_transports(create_web_socket_transports().await).await;

    println!("--- Running example with QUIC Transports ---");
    run_with_transports(create_quic_transports().await).await;
}

async fn run_with_transports<T: Transport + Send + Sync + 'static>(
    (client_transport, server_transport): (T, T),
) {
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

        let book_service_module = client_port
            .load_module::<BookServiceClient>("BookService")
            .await
            .unwrap();

        let get_book_payload = GetBookRequest { isbn: 1000 };

        let response = book_service_module.get_book(get_book_payload).await;

        println!("> Got Book: {:?}", response);

        assert_eq!(response.isbn, 1000);
        assert_eq!(response.title, "Rust: crash course");
        assert_eq!(response.author, "mr steve");
    });

    let server_handle = tokio::spawn(async {
        let ctx = MyExampleContext {
            hardcoded_database: create_db(),
        };

        // 2- Create Server with Transport
        let mut server = RpcServer::create(ctx);
        // 3- Server listen to Create Port request
        server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceCodeGen::register_service(port, book_service::BookService {})
        });

        server.attach_transport(server_transport);

        server.run().await;
    });

    client_handle.await.unwrap();
    server_handle.await.unwrap();
}
