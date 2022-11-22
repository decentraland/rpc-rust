use rpc_rust::server::{RpcServer, RpcServerPort};
use rpc_rust::transports::Transport;
use rpc_rust::transports::memory::MemoryTransport;

struct BookContext {
    books: Vec<String>,
}

#[tokio::test(flavor = "multi_thread", worker_threads=1)]
async fn call_procedure() {
    async_scoped::TokioScope::scope_and_block(|scope| {
        // 1- Create Transport
        let (_client_transport, server_transport) = MemoryTransport::create();

        scope.spawn(async move {
            _client_transport.send(vec![0]).await;
        });
        
        scope.spawn(async {
            // 2- Create Server with Transport
            let mut server = RpcServer::create();
            // 3- Server listen to Create Port request
            server.set_handler(|port: &mut RpcServerPort| {
                println!("Port {} created!", port.name);
                port.register("GetBook".to_string(), |request| {
                    //return GetBookResponse();
                });
            });

            server.attach_transport(server_transport);

            server.run().await;
        });
    });
    /*let client_handle = tokio::spawn(||{
        // 4- Client -> Create Port
        let client = RpcClient::create(client_transport);
        let port = client.createPort("port-name").await;

        // 5- Load Module
        let book_module = port.load_module("BookService").await;
        // 6- Call Procedure
        let payload = BookRequest.toBytes();// GetBook as bytes
        let response = book_module.callUnary("GetBook", payload).await;

        let book_service = ClientBookServiceCodeGen.new(book_module);
        let response = book_service.GetBook(payload).await;
        // response should be GetBookResponse as bytes
    });*/
}
