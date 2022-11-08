use rpc_rust::protocol::index::CreatePort;
use rpc_rust::server::{RpcServer, RpcServerPort};
use rpc_rust::transports::memory::MemoryTransport;
use rpc_rust::transports::Transport;

struct BookContext
{
    books: Vec<String>
}

#[tokio::test]
async fn call_procedure() {
    // 1- Create Transport

    let (client_transport, server_transport) = MemoryTransport::create();
    
    // 2- Create Server with Transport
    let mut server = RpcServer::create();

    // 3- Server listen to Create Port request
    let handler = Box::new(|port: RpcServerPort|{
        println!("Port created!");
        /*port.register("GetBook", |request, context|{
            return GetBookResponse();
        });*/
    });
    server.set_handler(handler);
    
    server.attach_transport(server_transport)

    server.run().await

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
