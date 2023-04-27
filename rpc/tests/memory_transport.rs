use dcl_rpc::{
    self,
    transports::{memory::MemoryTransport, Transport, TransportError},
};

#[tokio::test]
async fn ping_pong() {
    let (client, server) = MemoryTransport::create();

    let input = vec![1, 2, 3];
    let _ = client.send(input.clone()).await;

    let message = server.receive().await.expect("can receive");
    assert_eq!(&message, &input);
    let _ = server.send(message).await;

    let message = client.receive().await.expect("can receive");
    assert_eq!(&message, &input);

    client.close().await;
    let close_event = client.receive().await.unwrap_err();
    assert!(matches!(close_event, TransportError::Closed));
}
