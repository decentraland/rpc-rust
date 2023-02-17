use prost::Message;

use super::{RpcMessageHeader, RpcMessageTypes};

/// Build message identifier from type and number, and returns one number which it's the `message_identifier`
///
/// `message_identifier` packs two numbers:
///
/// Bits from 1 to 28 correspond to the sequential message id (analogous to JSON-RPC 2)
///
/// Bits from 28 to 32 correspond to message_type
pub fn build_message_identifier(message_type: u32, message_number: u32) -> u32 {
    ((message_type & 0xf) << 27) | (message_number & 0x07ffffff)
}

/// Parse message type and number from message identifier
///
/// Do the inverse calculation of `build_message_identifier`
pub fn parse_message_identifier(value: u32) -> (u32, u32) {
    ((value >> 27) & 0xf, value & 0x07ffffff)
}

/// Parse `data` to message type and message identifier
pub fn parse_header(data: &[u8]) -> Option<(RpcMessageTypes, u32)> {
    let message_header = RpcMessageHeader::decode(data).ok()?;
    let (message_type, message_number) =
        parse_message_identifier(message_header.message_identifier);
    let rpc_message_type = RpcMessageTypes::from_i32(message_type as i32)?;
    Some((rpc_message_type, message_number))
}

/// Parse protocol message from bytes
///
/// Returns None when message can't be parsed or should not do it
pub fn parse_protocol_message<R: Message + Default>(data: &[u8]) -> Option<(u32, u32, R)> {
    let (message_type, message_number) = parse_header(data).unwrap();

    if matches!(
        message_type,
        RpcMessageTypes::Empty | RpcMessageTypes::ServerReady
    ) {
        return None;
    }

    let message = R::decode(data);

    match message {
        Ok(message) => Some((message_type as u32, message_number, message)),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::rpc_protocol::*;
    use prost::Message;

    use super::{build_message_identifier, parse_protocol_message};

    #[test]
    fn test_parse_protocol_message() {
        let port = CreatePort {
            message_identifier: build_message_identifier(RpcMessageTypes::CreatePort as u32, 1),
            port_name: "port_name".to_string(),
        };

        let vec = port.encode_to_vec();

        let parse_back = parse_protocol_message::<CreatePortResponse>(&vec);

        assert!(parse_back.is_some());
        let parse_back = parse_back.unwrap();
        assert_eq!(parse_back.0, RpcMessageTypes::CreatePort as u32);
        // parse_protocol_message dont add the port_id, just parse it
        assert_eq!(parse_back.2.port_id, 0);
    }
}
