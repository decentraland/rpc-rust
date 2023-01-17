use tokio::sync::mpsc::UnboundedReceiver;

pub mod client;
pub mod server;

pub type ServerStreamResponse<T> = UnboundedReceiver<T>;
