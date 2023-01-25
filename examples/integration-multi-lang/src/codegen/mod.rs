use rpc_rust::stream_protocol::Generator;

pub mod server;

pub type ServerStreamResponse<T> = Generator<T>;
pub type ClientStreamRequest<T> = Generator<T>;
