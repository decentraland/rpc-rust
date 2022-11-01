use rpc_rust::protocol::index::CreatePort;
use rpc_rust::server::{RpcServer, RpcServerPort};
use rpc_rust::transports::memory::MemoryTransport;
use rpc_rust::transports::Transport;

#[tokio::test]
async fn call_procedure() {
    // 1- Create Transport
    let (client_transport, server_transport) = MemoryTransport::create();
    // 2- Create Server with Transport
    let mut server = RpcServer::create();
    
    // 3- Client -> Create Port
    // client_transport.send();

    // 4- Server listen to Create Port request
    server.set_handler(Box::new(|port: RpcServerPort|{
        println!("Port created!");
    }));
    server.attach_transport(server_transport, context);
    // 4- Load Module
    let book_service = client.load_module("BookService");
    // 5- Call Procedure
    let bytes = // GetBook as bytes 
    let response = book_service.callUnary("GetBook", bytes);
    // response should be GetBookResponse as bytes
}
