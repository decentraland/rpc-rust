use rpc_rust::{
    self,
    transports::{memory::MemoryTransport, Transport, TransportEvent},
};

#[tokio::test]
async fn ping_pong() {
    let (mut client, mut server) = MemoryTransport::create();

    let input = vec![1, 2, 3];
    let _ = client.send(input.clone()).await;

    let message = server.receive().await.expect("can receive");
    assert!(matches!(message, TransportEvent::Message(_)));
    if let TransportEvent::Message(data) = message {
        assert_eq!(&data, &input);
        let _ = server.send(data).await;
    }

    let message = client.receive().await.expect("can receive");
    assert!(matches!(message, TransportEvent::Message(_)));
    if let TransportEvent::Message(data) = message {
        assert_eq!(&data, &input);
    }

    client.close();
    let close_event = client.receive().await.expect("can receive");
    assert!(matches!(close_event, TransportEvent::Close));
}
