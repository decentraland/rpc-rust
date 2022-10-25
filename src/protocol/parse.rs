use protobuf::{Message, ProtobufEnum};

use super::index::{
    CreatePort, CreatePortResponse, DestroyPort, RemoteError, Request, RequestModule,
    RequestModuleResponse, Response, RpcMessageHeader, RpcMessageTypes, StreamMessage,
};

/// Parse message type and number from message identifier
pub fn parse_message_identifier(value: u32) -> (u32, u32) {
    ((value >> 27) & 0xf, value & 0x07ffffff)
}

// Parse data to message type and message identifier
pub fn parse_header(data: &[u8]) -> Option<(RpcMessageTypes, u32)> {
    let message_header = RpcMessageHeader::parse_from_bytes(data).ok()?;
    let (message_type, message_number) =
        parse_message_identifier(message_header.get_message_identifier());
    let rpc_message_type = RpcMessageTypes::from_i32(message_type as i32)?;
    Some((rpc_message_type, message_number))
}

/// Parse protocol message from bytes
/// Returns None when message can't be parsed or should not do it
pub fn parse_protocol_message(data: &[u8]) -> Option<(u32, Box<dyn Message>, u32)> {
    let message_header = RpcMessageHeader::parse_from_bytes(data).ok()?;
    let (message_type, message_number) =
        parse_message_identifier(message_header.get_message_identifier());
    let rpc_message_type = RpcMessageTypes::from_i32(message_type as i32)?;

    let message: Box<dyn Message> = match rpc_message_type {
        RpcMessageTypes::RpcMessageTypes_REQUEST => Box::new(Request::parse_from_bytes(data).ok()?),
        RpcMessageTypes::RpcMessageTypes_RESPONSE => {
            Box::new(Response::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_CREATE_PORT_RESPONSE => {
            Box::new(CreatePortResponse::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_STREAM_MESSAGE => {
            Box::new(StreamMessage::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_STREAM_ACK => {
            Box::new(StreamMessage::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_CREATE_PORT => {
            Box::new(CreatePort::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE => {
            Box::new(RequestModule::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_REQUEST_MODULE_RESPONSE => {
            Box::new(RequestModuleResponse::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_REMOTE_ERROR_RESPONSE => {
            Box::new(RemoteError::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_DESTROY_PORT => {
            Box::new(DestroyPort::parse_from_bytes(data).ok()?)
        }
        RpcMessageTypes::RpcMessageTypes_EMPTY | RpcMessageTypes::RpcMessageTypes_SERVER_READY => {
            return None
        }
    };

    Some((message_type, message, message_number))
}
