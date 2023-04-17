//! Parsing functions for the messages that are sent on communications between two different ends using the Decentraland RPC implementation.
use super::{RemoteError, RpcMessageHeader, RpcMessageTypes};
use prost::Message;

/// Build message identifier from type and number, and returns one number which it's the `message_identifier`
///
/// `message_identifier` packs two numbers:
///
/// Bits from 1 to 28 correspond to the sequential message number (analogous to JSON-RPC 2)
///
/// Bits from 28 to 32 correspond to message type
pub fn build_message_identifier(message_type: u32, message_number: u32) -> u32 {
    ((message_type & 0xf) << 27) | (message_number & 0x07ffffff)
}

/// Parse message type and number from message identifier
///
/// Do the inverse calculation of `build_message_identifier`
pub fn parse_message_identifier(value: u32) -> (u32, u32) {
    ((value >> 27) & 0xf, value & 0x07ffffff)
}

/// Decode bytes into [`RpcMessageHeader`] to get message type and message number
/// which are two number that compose the `message_identifier` in the [`RpcMessageHeader`] struct
pub fn parse_header(data: &[u8]) -> Option<(RpcMessageTypes, u32)> {
    let message_header = RpcMessageHeader::decode(data).ok()?;
    let (message_type, message_number) =
        parse_message_identifier(message_header.message_identifier);
    let rpc_message_type = RpcMessageTypes::from_i32(message_type as i32)?;
    Some((rpc_message_type, message_number))
}

/// Errors produced by [`parse_protocol_message`]
#[derive(Debug)]
pub enum ParseErrors {
    /// Found a RemoteError message instead of the given type parameter
    IsARemoteError((u32, RemoteError)),
    /// The bytes failed decoding into the given type
    DecodingFailed,
    /// It's an error when the [`RpcMessageHeader`] has: [`RpcMessageTypes::Empty`] or
    /// [`RpcMessageTypes::ServerReady`] as `message_type` in its identifier. These message types don't have
    /// a message itself, they come as a [`RpcMessageHeader`]
    NotMessageType,
    /// The function, first, tries to decode the bytes into [`RpcMessageHeader`] to get the `message_type` and `message_number`
    ///
    /// If it fails, the bytes are fully invalid.
    InvalidHeader,
}

type ParseMessageResult<R> = Result<(u32, u32, R), ParseErrors>;

/// Parse protocol message from bytes
///
/// A type parameter `R` (generic) is passed to try to decode the bytes into the given type.
///
/// If all went well, an `Ok(Option<u32, u32, R>)` is returned.
///
/// If the bytes cannot be decoded into the given type or the [`RpcMessageHeader`]'s message type of the given bytes is a [`RpcMessageTypes::Empty`] or a [`RpcMessageTypes::ServerReady`], a `Ok(None)`  is returned.
///
/// Also, if the [`RpcMessageHeader`] cannot be gotten from the given bytes, a `Ok(None)` is returned because the bytes are fully invalid.
///
/// If the [`RpcMessageHeader`]'s message type of the given bytes is a [`RpcMessageTypes::RemoteErrorResponse`], an Err((u32, [`RemoteError`])) is returned.
///
pub fn parse_protocol_message<R: Message + Default>(data: &[u8]) -> ParseMessageResult<R> {
    let (message_type, message_number) = match parse_header(data) {
        Some(header) => header,
        None => return Err(ParseErrors::InvalidHeader),
    };

    if matches!(message_type, RpcMessageTypes::RemoteErrorResponse) {
        match RemoteError::decode(data) {
            Ok(remote_error) => {
                return Err(ParseErrors::IsARemoteError((message_number, remote_error)))
            }
            Err(_) => {
                let mut remote_error_default = RemoteError::default();
                fill_remote_error(&mut remote_error_default, message_number);

                return Err(ParseErrors::IsARemoteError((
                    message_number,
                    remote_error_default,
                )));
            }
        }
    }

    if matches!(
        message_type,
        RpcMessageTypes::Empty | RpcMessageTypes::ServerReady
    ) {
        return Err(ParseErrors::NotMessageType);
    }

    let message = R::decode(data);

    match message {
        Ok(message) => Ok((message_type as u32, message_number, message)),
        Err(_) => Err(ParseErrors::DecodingFailed),
    }
}

/// This functions works for filling a [`RemoteError`]. This is needed because:
///
/// When a server procedure returns a custom type as an error, which has to implement [`RemoteErrorResponse`](`super::RemoteErrorResponse`).
///
/// That custom type is then turned into a [`RemoteError`] but without a "true" value (`0`) in the `message_identifier` field
///
/// Because at the moment of conversion, the `message_number` is not available so the [`RemoteError`] has to be filled then.
///
pub fn fill_remote_error(remote_error: &mut RemoteError, message_number: u32) {
    remote_error.message_identifier =
        build_message_identifier(RpcMessageTypes::RemoteErrorResponse as u32, message_number);
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

        let parse_back = parse_protocol_message::<CreatePortResponse>(&vec).unwrap();

        assert_eq!(parse_back.0, RpcMessageTypes::CreatePort as u32);
        // parse_protocol_message dont add the port_id, just parse it
        assert_eq!(parse_back.2.port_id, 0);
    }

    #[test]
    fn test_remote_error_in_parse_protocol_message() {
        let remote_error = RemoteError {
            message_identifier: build_message_identifier(
                RpcMessageTypes::RemoteErrorResponse as u32,
                1,
            ),
            error_code: 400,
            error_message: "Bad request error".to_string(),
        };
        let vec = remote_error.encode_to_vec();

        let parse_back = parse_protocol_message::<CreatePortResponse>(&vec).unwrap_err();
        match parse_back {
            parse::ParseErrors::IsARemoteError((message_number, remote_error_produced)) => {
                assert_eq!(message_number, 1);
                assert_eq!(remote_error, remote_error_produced);
            }
            _ => panic!(),
        }
    }
}
