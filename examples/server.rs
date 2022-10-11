//! A simple echo server.
//!
//! You can test this out by running:
//!
//!     cargo run --example server 127.0.0.1:12345
//!
//! And then in another window run:
//!
//!     cargo run --example client ws://127.0.0.1:12345/

use std::{env, io::Error};

use futures_util::{future, StreamExt, TryStreamExt};
use log::info;
use protobuf::Message as ProtoMessage;
use rpc_rust::protocol::index::Request;
use rpc_rust::protocol::index::Response;
use rpc_rust::protocol::parse::parse_header;
use rpc_rust::protocol::parse::parse_message_identifier;
use rpc_rust::protocol::parse::parse_protocol_message;
use tokio::net::{TcpListener, TcpStream};
use tungstenite::Message;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::try_init();
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    info!("Peer address: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    info!("New WebSocket connection: {}", addr);

    let (write, read) = ws_stream.split();
    // We should not forward messages other than text or binary.
    read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
        .map(|message| async {
            if let Ok(message) = message {
                let data = message.into_data();
                let messageHeader = parse_header(&data).unwrap();
                match messageHeader.0 {
                    rpc_rust::protocol::index::RpcMessageTypes::RpcMessageTypes_REQUEST => {
                        let request = Request::parse_from_bytes(&data).unwrap();
                        let response = Response::default();
                        response.set_payload(request.get_payload().to_vec());
                        return Message::binary(response.write_to_bytes().unwrap());
                    }
                    _ => todo!(),
                }
            }
            return Message::binary(vec![0]);
        })
        .forward(write)
        .await
        .expect("Failed to forward messages")
}
