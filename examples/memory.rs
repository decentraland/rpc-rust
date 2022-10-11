use tokio::sync::mpsc::channel;

/// In memory ping pong using channels and Vec<u8> as the type of the message
#[tokio::main]
async fn main() {
    // A -> B
    // B -> A

    let (client_sender, mut sever_receiver) = channel::<Vec<u8>>(1);
    let (server_sender, mut client_receiver) = channel::<Vec<u8>>(1);

    let input = "123".as_bytes().to_vec();

    client_sender.send(input).await.expect("can send input");
    let message = sever_receiver.recv().await.expect("can receive message");
    println!("{:?}", String::from_utf8(message.clone()));

    server_sender.send(message).await.expect("can send message");
    let message = client_receiver.recv().await.expect("can receive message");
    println!("{:?}", String::from_utf8(message.clone()));
}

// Next steps:
// Trait like https://github.com/decentraland/rpc/blob/main/src/transports/Memory.ts
// Implement it with channels
// Create test with literal input
// Create test of memory transports
// Create Request <-> Response test
