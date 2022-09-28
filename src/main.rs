use protobuf::Message;

use crate::protocol::index::RpcMessageHeader;

pub mod protocol;

fn main() {
    let mut message = RpcMessageHeader::new();

    message.message_identifier = 35;

    let res = message.write_to_bytes();
    if let Ok(res) = res {
        println!("Hello, world! {}", res.len());
    }
    println!("Hello, world!");
}
