use dcl_rpc::stream_protocol::Generator;

pub mod client;
pub mod server;

pub type ServerStreamResponse<T> = Generator<T>;
pub type ClientStreamRequest<T> = Generator<T>;
