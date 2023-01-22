use std::{env, time::Duration};

use integration::{
    codegen::{
        client::{BookServiceClient, BookServiceClientInterface},
        server::BookServiceCodeGen,
    },
    service::book_service,
    setup_quic::{configure_client, generate_self_signed_cert},
    Book, GetBookRequest, MyExampleContext, QueryBooksRequest,
};
use quinn::ServerConfig;
use rpc_rust::{
    client::RpcClient,
    server::{RpcServer, RpcServerPort},
    stream_protocol::Generator,
    transports::{
        self, memory::MemoryTransport, quic::QuicTransport, web_socket::WebSocketTransport,
        Transport,
    },
};
use tokio::{join, time::sleep};

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
    let args: Vec<String> = env::args().collect();
    let example = args.get(1);
    if let Some(example) = example {
        if example == "memory" {
            println!("--- Running example with Memory Transports ---");
            run_with_transports(create_memory_transports()).await;
        } else if example == "ws" {
            println!("--- Running example with Web Socket Transports ---");
            run_with_transports(create_web_socket_transports().await).await;
        } else if example == "quic" {
            println!("--- Running example with QUIC Transports ---");
            run_with_transports(create_quic_transports().await).await;
        }
    } else {
        println!("--- Running example with Memory Transports ---");
        run_with_transports(create_memory_transports()).await;
        println!("--- Running example with Web Socket Transports ---");
        run_with_transports(create_web_socket_transports().await).await;
        println!("--- Running example with QUIC Transports ---");
        run_with_transports(create_quic_transports().await).await;
    }
}

async fn run_with_transports<T: Transport + Send + Sync + 'static>(
    (client_transport, server_transport): (T, T),
) {
    // Another client to test multiple transports server feature
    let client_2_transport = create_memory_transports();
    let client_handle = tokio::spawn(async move {
        let mut client = RpcClient::new(client_transport).await.unwrap();

        let mut client_2 = RpcClient::new(client_2_transport.0).await.unwrap();

        println!("> Creating Port");
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
        println!("> Port created");

        println!("> Creating Port 2");
        let client_port_2 = match client_2.create_port("TEST_PORT_2").await {
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

        assert_eq!(client_port_2.port_name, "TEST_PORT_2");
        println!("> Port 2 created");

        println!("> Calling load module");
        let book_service_module = client_port
            .load_module::<BookServiceClient>("BookService")
            .await
            .unwrap();

        println!("> Calling load module for client 2");
        let book_service_module_2 = client_port_2
            .load_module::<BookServiceClient>("BookService")
            .await
            .unwrap();

        println!("> Unary > Request > GetBook");

        let get_book_payload = GetBookRequest { isbn: 1000 };
        let response = book_service_module.get_book(get_book_payload).await;

        println!("> Unary > Response > GetBook: {:?}", response);

        assert_eq!(response.isbn, 1000);
        assert_eq!(response.title, "Rust: crash course");
        assert_eq!(response.author, "mr steve");

        println!("> Client 2 > Unary > Request > GetBook");

        let get_book_payload = GetBookRequest { isbn: 1004 };
        let response = book_service_module_2.get_book(get_book_payload).await;

        println!("> Client 2 > Unary > Response > GetBook: {:?}", response);

        assert_eq!(response.isbn, 1004);
        assert_eq!(response.title, "Smart Contracts 101");
        assert_eq!(response.author, "buterin");

        drop(client_2);

        println!("> GetBook: Concurrent Example");

        let get_book_payload = GetBookRequest { isbn: 1000 };
        let get_book_payload_2 = GetBookRequest { isbn: 1001 };

        join!(
            book_service_module.get_book(get_book_payload),
            book_service_module.get_book(get_book_payload_2)
        );

        println!("> Server Streams > Request > QueryBooks");

        let query_books_payload = QueryBooksRequest {
            author_prefix: "mr".to_string(),
        };

        let mut response_stream = book_service_module.query_books(query_books_payload).await;

        while let Some(book) = response_stream.next().await {
            println!("> Server Streams > Response > QueryBooks {:?}", book)
        }

        println!("> Client Streams > Request > GetBookStream");
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            for _ in 0..4 {
                sleep(Duration::from_millis(300)).await;
                println!("> Client Stream > GetBookStream > Sending stream payload");
                generator_yielder
                    .insert(GetBookRequest { isbn: 1000 })
                    .await
                    .unwrap();
            }
        });
        let response = book_service_module.get_book_stream(generator).await;

        println!("> Client Streams > Response > GetBookStream {response:?}");

        println!("> BiDir Streams > Request > QueryBooksStream");
        let (generator, generator_yielder) = Generator::create();
        tokio::spawn(async move {
            for i in 0..4 {
                sleep(Duration::from_millis(300)).await;
                println!("> BiDir Stream > QueryBooksStream > Sending stream payload");
                generator_yielder
                    .insert(GetBookRequest { isbn: (1000 + i) })
                    .await
                    .unwrap();
            }
        });
        let mut response = book_service_module.query_books_streams(generator).await;

        while let Some(book) = response.next().await {
            println!("> BiDir Streams > Response > QueryBooksStream {:?}", book)
        }
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

        match server.attach_transport(server_transport) {
            Ok(_) => {
                println!("> RpcServer > first transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }

        match server.attach_transport(client_2_transport.1) {
            Ok(_) => {
                println!("> RpcServer > second transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }

        server.run().await;
    });

    client_handle.await.unwrap();
    server_handle.await.unwrap();
}
