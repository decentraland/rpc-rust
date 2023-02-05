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
        self,
        memory::MemoryTransport,
        quic::QuicTransport,
        web_socket::{WebSocketClient, WebSocketServer, WebSocketTransport},
        Transport,
    },
};
use tokio::{join, select, time::sleep};
use tokio_util::sync::CancellationToken;

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

#[allow(dead_code)]
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
            run_memory_transport().await;
        } else if example == "ws" {
            println!("--- Running example with Web Socket Transports ---");
            run_ws_tramsport().await;
        }
        // TODO: fix QUIC transport (similar to ws fix)
        // } else if example == "quic" {
        //     // println!("--- Running example with QUIC Transports ---");
        //     // run_with_transports(TransportType::QUIC).await;
        //}
    } else {
        println!("--- Running example with Memory Transports ---");
        run_memory_transport().await;
        println!("--- Running example with Web Socket Transports ---");
        run_ws_tramsport().await;
        // TODO: fix QUIC transport (similar to ws fix)
        // println!("--- Running example with QUIC Transports ---");
        // run_with_transports(TransportType::QUIC).await;
    }
}

async fn run_memory_transport() {
    let (client_transport, server_transport) = create_memory_transports();
    let cancellation_token = CancellationToken::new();
    // Another client to test multiple transports server feature
    let (client_2_transport, server_2_transport) = create_memory_transports();
    let cloned_token = cancellation_token.clone();

    let client_handle = tokio::spawn(async move {
        handle_client_connection((client_transport, client_2_transport)).await;
        cloned_token.cancel();
    });

    let server_handle = tokio::spawn(async {
        let ctx = MyExampleContext {
            hardcoded_database: create_db(),
        };

        let mut server = RpcServer::create(ctx);
        server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceCodeGen::register_service(port, book_service::BookService {})
        });

        // Not needed to use the server events sender it can be attached direcrly
        // Since it doesn't have to anything or wait anything in background
        match server.attach_transport(server_transport) {
            Ok(_) => {
                println!("> RpcServer > first transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }

        match server.attach_transport(server_2_transport) {
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
    select! {
        _ = cancellation_token.cancelled() => {
            println!("Example finished manually")
        },
        _ =  server_handle => {
            println!("Server terminated unexpectedly")
        }
    }
}

async fn run_ws_tramsport() {
    let (ws_server, mut connection_listener) = WebSocketServer::new("127.0.0.1:8080");

    ws_server.listen().await.unwrap();

    let cancellation_token = CancellationToken::new();
    // Another client to test multiple transports server feature
    let cloned_token = cancellation_token.clone();

    let client_handle = tokio::spawn(async move {
        let client_connection = WebSocketClient::connect("ws://127.0.0.1:8080")
            .await
            .unwrap();

        let client_transport = WebSocketTransport::new(client_connection);

        let client_connection = WebSocketClient::connect("ws://127.0.0.1:8080")
            .await
            .unwrap();
        let client_2_transport = WebSocketTransport::new(client_connection);

        handle_client_connection((client_transport, client_2_transport)).await;

        cloned_token.cancel();
    });

    let server_handle = tokio::spawn(async {
        let ctx = MyExampleContext {
            hardcoded_database: create_db(),
        };

        let mut server = RpcServer::create(ctx);
        server.set_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceCodeGen::register_service(port, book_service::BookService {})
        });

        // It has to use the server events sender to attach transport because it has to wait for client connections
        // and keep waiting for new ones
        let server_events_sender = server.get_server_events_sender();
        tokio::spawn(async move {
            while let Some(Ok(connection)) = connection_listener.recv().await {
                let transport = WebSocketTransport::new(connection);
                match server_events_sender.send_attach_transport(transport) {
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

        server.run().await;
    });

    client_handle.await.unwrap();
    select! {
        _ = cancellation_token.cancelled() => {
            println!("Example finished manually")
        },
        _ =  server_handle => {
            println!("Server terminated unexpectedly")
        }
    }
}

async fn handle_client_connection<T: Transport + Send + Sync + 'static>(
    (client_transport, client_2_transport): (T, T),
) {
    let mut client = RpcClient::new(client_transport).await.unwrap();

    let mut client_2 = RpcClient::new(client_2_transport).await.unwrap();

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
}
