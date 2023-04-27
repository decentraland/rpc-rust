use dcl_rpc::{
    client::RpcClient,
    server::{RpcServer, RpcServerPort},
    stream_protocol::Generator,
    transports::{
        memory::MemoryTransport,
        web_socket::{WebSocketClient, WebSocketServer, WebSocketTransport},
        Transport,
    },
};
use integration::{
    service::book_service, Book, BookServiceClient, BookServiceClientDefinition,
    BookServiceRegistration, GetBookRequest, MyExampleContext, QueryBooksRequest,
};
use std::{env, sync::Arc, time::Duration};
use tokio::{join, select, sync::RwLock, time::sleep};
use tokio_util::sync::CancellationToken;

// An in-memory database for the examples
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

/// Function to creates the memory transport and returns a tuple of two transports, first one for the client and second one for the server.
fn create_memory_transports() -> (MemoryTransport, MemoryTransport) {
    MemoryTransport::create()
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
            run_ws_transport().await;
        } else if example == "dyn" {
            println!("--- Running example with &dyn Transport ---");
            run_with_dyn_transport().await;
        }
    } else {
        println!("--- Running example with Memory Transports ---");
        run_memory_transport().await;
        println!("--- Running example with Web Socket Transports ---");
        run_ws_transport().await;
        // Giving time to the OS to terminate the binding to the 8080 port used by the both examples.
        // The listener abort on dropping the ws server.
        sleep(Duration::from_secs(1)).await;
        //
        println!("--- Running example with &dyn Transport ---");
        run_with_dyn_transport().await;
    }
}

/// This example runs the RpcServer and RpcClients using Memory as a transport for messages.
async fn run_memory_transport() {
    let (client_transport, server_transport) = create_memory_transports();
    let cancellation_token = CancellationToken::new();
    // Another client to test multiple transports server feature
    let (client_2_transport, server_2_transport) = create_memory_transports();
    let cloned_token = cancellation_token.clone();

    // Runs the client in a background task. It would be impossible to run this sequentially since the clients and the server are in the same process.
    let client_handle = tokio::spawn(async move {
        handle_client_connection((client_transport, client_2_transport)).await;
        cloned_token.cancel();
    });

    // Spawining the server in a tokio task
    let server_handle = tokio::spawn(async {
        // 1. Creates a context for the server. The server receives a generic but should be the same context you pass to the MyBookService when you implement the BookServiceServer trait
        // you could put whatever you want in the context. For example: A database component to use it inside of the procedures logic.
        let ctx = MyExampleContext {
            hardcoded_database: RwLock::new(create_db()),
        };

        // 2. Creates the RpcServer passing its context.
        let mut server = RpcServer::create(ctx);

        // 3. Sets a handler for the port creation. Here you could put a condition for registering a service depending on the port name.
        // Multiple services can live in a single a RpcServer.
        server.set_module_registrator_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceRegistration::register_service(port, book_service::MyBookService {})
        });

        // 4. Attaches the server-side transports to the RpcServer
        // Not needed to use the ServerEventsSender, it can be attached direcrly as we are using memory transport.
        // Since it doesn't have to wait anything in background

        // The transport of the first client
        match server.attach_transport(Arc::new(server_transport)) {
            Ok(_) => {
                println!("> RpcServer > first transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }
        // Attach the server-side transport for the second client
        match server.attach_transport(Arc::new(server_2_transport)) {
            Ok(_) => {
                println!("> RpcServer > second transport attached successfully");
            }
            Err(_) => {
                println!("> RpcServer > unable to attach transport");
                panic!()
            }
        }

        // 5. Run the server and listen for messages from the clients. This blocks the program (in this case this specific task)
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

/// This example runs the RpcServer and RpcClients using WebSockets as a transport for messages
async fn run_ws_transport() {
    // 1. Creates the WebSocket server to listen for WS connections.
    let mut ws_server = WebSocketServer::new("127.0.0.1:8080");

    // 2. Makes the WebSocket server start listening for connections. Not blocking as it'll be listening in the background
    let mut connection_listener = ws_server.listen().await.unwrap();

    // Cancellation tokens for stop gracefully the example when it runs
    let cancellation_token = CancellationToken::new();
    let cloned_token = cancellation_token.clone();

    // Runs the client in a background task. It would be impossible to run this sequentially since the clients and the server are in the same process.
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

    // Runs the RpcServer in a backgroun task
    let server_handle = tokio::spawn(async {
        // 3. Creates a context for the server. The server receives a generic but should be the same context you pass to the MyBookService when you implement the BookServiceServer trait
        // you could put whatever you want in the context. For example: A database component to use it inside of the procedures logic.
        let ctx = MyExampleContext {
            hardcoded_database: RwLock::new(create_db()),
        };

        // 4. Creates the RpcServer passing its context.
        let mut server = RpcServer::create(ctx);

        // 5. Sets a handler for the port creation. Here you could put a condition for registering a service depending on the port name.
        // Multiple services can live in a single a RpcServer.
        server.set_module_registrator_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceRegistration::register_service(port, book_service::MyBookService {})
        });

        // 6. Spawns a background task to receive the WS connections that the WebSocket server are accepting to send it through a channel to the RpcServer. A RpcServer could have multiple clients
        // (so multiple transports) so we have to keep sending the new ones clients, so it exposes a channel sender to send events, and one event is a new transport that it should attach.
        // It *must* use the server events sender to attach transport because it should wait for client connections and keep waiting for new ones.
        let server_events_sender = server.get_server_events_sender();
        tokio::spawn(async move {
            while let Some(Ok(connection)) = connection_listener.recv().await {
                let transport = WebSocketTransport::new(connection);
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

        // 7. Run the server and listen for messages from the clients. This blocks the program (in this case this specific task)
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

/// This example wants to show you that it's possible to use multiple type of transports, we don't know if there is a real use case but it's possible.
async fn run_with_dyn_transport() {
    // 1. Creates the WebSocket server to listen for WS connections.
    let mut ws_server = WebSocketServer::new("127.0.0.1:8080");

    // 2. Makes the WebSocket server start listening for connections. Not blocking as it'll be listening in the background
    let mut connection_listener = ws_server.listen().await.unwrap();

    // Cancellation tokens for stop gracefully the example when it runs
    let cancellation_token = CancellationToken::new();
    let cloned_token = cancellation_token.clone();

    // 3. Creates a memory transport due to the dyn example
    let (client_memory_transport, server_memory_transport) = MemoryTransport::create();

    let client_handle = tokio::spawn(async move {
        let client_connection = WebSocketClient::connect("ws://127.0.0.1:8080")
            .await
            .unwrap();

        let client_transport = WebSocketTransport::new(client_connection);

        handle_client_connection((client_transport, client_memory_transport)).await;

        cloned_token.cancel();
    });

    let server_handle = tokio::spawn(async {
        // 3. Creates a context for the server. The server receives a generic but should be the same context you pass to the MyBookService when you implement the BookServiceServer trait
        // you could put whatever you want in the context. For example: A database component to use it inside of the procedures logic.
        let ctx = MyExampleContext {
            hardcoded_database: RwLock::new(create_db()),
        };

        // 4. Creates the RpcServer passing its context.
        let mut server = RpcServer::create(ctx);

        // 5. Sets a handler for the port creation. Here you could put a condition for registering a service depending on the port name.
        // Multiple services can live in a single a RpcServer.
        server.set_module_registrator_handler(|port: &mut RpcServerPort<MyExampleContext>| {
            BookServiceRegistration::register_service(port, book_service::MyBookService {})
        });

        // Cast Arc<> to use multiple transport types in the server
        let server_memory_transport_to_arc: Arc<dyn Transport> = Arc::new(server_memory_transport);

        // 6. Attach the dyn transport directly since we don't have to wait anything here, it's a memory transport.
        server
            .attach_transport(server_memory_transport_to_arc)
            .unwrap();

        // 7. Spawns a background task to receive the WS connections that the WebSocket server are accepting to send it through a channel to the RpcServer. A RpcServer could have multiple clients
        // (so multiple transports) so we have to keep sending the new ones clients, so it exposes a channel sender to send events, and one event is a new transport that it should attach.
        // It *must* use the server events sender to attach transport because it should wait for client connections and keep waiting for new ones.
        let server_events_sender = server.get_server_events_sender();
        tokio::spawn(async move {
            while let Some(Ok(connection)) = connection_listener.recv().await {
                let transport = WebSocketTransport::new(connection);
                let transport_to_arc: Arc<dyn Transport> = Arc::new(transport);
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

        // 8. Run the server and listen for messages from the clients.
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

/// Creates client and send request to the `RpcServer`
async fn handle_client_connection<T: Transport + 'static, T2: Transport + 'static>(
    (client_transport, client_2_transport): (T, T2),
) {
    // 1. Creates the first client for the server
    let mut client = RpcClient::new(client_transport).await.unwrap();

    // 2. Creates a second client for the server
    let mut client_2 = RpcClient::new(client_2_transport).await.unwrap();

    println!("> Creating Port");
    // 3. Creates a client port for the first client. This port can load the modules that the server handler sets when a client creates a port.
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
    // 3. Creates a client port for the second client. This port can load the modules that the server handler sets when a client creates a port.
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
    // 4. Loads the module's procedures (service/api) and returning a client (BookServiceClient) to query the API
    let book_service_client = client_port
        .load_module::<BookServiceClient<T>>("BookService")
        .await
        .unwrap();

    println!("> Calling load module for client 2");
    let book_service_client_2 = client_port_2
        .load_module::<BookServiceClient<T2>>("BookService")
        .await
        .unwrap();

    // 5. Query the server through the client.
    println!("> Unary > Request > GetBook");

    let get_book_payload = GetBookRequest { isbn: 1000 };
    let response = book_service_client
        .get_book(get_book_payload)
        .await
        .unwrap(); // Should not fail

    println!("> Unary > Response > GetBook: {:?}", response);

    let get_book_payload = GetBookRequest { isbn: 2000 };
    let response_error = book_service_client
        .get_book(get_book_payload)
        .await
        .unwrap_err(); // SHOULD FAIL!

    println!(
        "> Unary > Response Error > RemoteError on GetBook: {:?}",
        response_error
    );

    assert_eq!(response.isbn, 1000);
    assert_eq!(response.title, "Rust: crash course");
    assert_eq!(response.author, "mr steve");

    println!("> Client 2 > Unary > Request > GetBook");

    let get_book_payload = GetBookRequest { isbn: 1004 };
    let response = book_service_client_2
        .get_book(get_book_payload)
        .await
        .unwrap(); // Should not fail

    println!("> Client 2 > Unary > Response > GetBook: {:?}", response);

    assert_eq!(response.isbn, 1004);
    assert_eq!(response.title, "Smart Contracts 101");
    assert_eq!(response.author, "buterin");

    drop(client_2);

    println!("> GetBook: Concurrent Example");

    let get_book_payload = GetBookRequest { isbn: 1000 };
    let get_book_payload_2 = GetBookRequest { isbn: 1001 };

    let (result_1, result_2) = join!(
        book_service_client.get_book(get_book_payload),
        book_service_client.get_book(get_book_payload_2)
    );

    println!(
        "> GetBook > Concurrent Example > Result 1: {:?}",
        result_1.unwrap()
    ); // Should not fail
    println!(
        "> GetBook > Concurrent Example > Result 2: {:?}",
        result_2.unwrap()
    ); // Should not fail

    println!("> Server Streams > Request > QueryBooks");

    let query_books_payload = QueryBooksRequest {
        author_prefix: "mr".to_string(),
    };

    let mut response_stream = book_service_client
        .query_books(query_books_payload)
        .await
        .unwrap(); // Should not fail

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
                .r#yield(GetBookRequest { isbn: 1000 })
                .await
                .unwrap();
        }
    });
    let response = book_service_client.get_book_stream(generator).await;

    println!("> Client Streams > Response > GetBookStream {response:?}");

    println!("> BiDir Streams > Request > QueryBooksStream");
    let (generator, generator_yielder) = Generator::create();
    tokio::spawn(async move {
        for i in 0..4 {
            sleep(Duration::from_millis(300)).await;
            println!("> BiDir Stream > QueryBooksStream > Sending stream payload");
            generator_yielder
                .r#yield(GetBookRequest { isbn: (1000 + i) })
                .await
                .unwrap();
        }
    });
    let mut response = book_service_client
        .query_books_stream(generator)
        .await
        .unwrap(); // Should not fail

    while let Some(book) = response.next().await {
        println!("> BiDir Streams > Response > QueryBooksStream {:?}", book)
    }
}
