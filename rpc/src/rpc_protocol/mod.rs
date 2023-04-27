//! Contains the types and functions needed to use the Decentraland RPC implementation.
pub mod parse;
// proto file definition doesn't have a package name, so it defaults to "_"
// TODO: scope the protcolol just to the crate
include!(concat!(env!("OUT_DIR"), "/_.rs"));

/// This trait should me implemented by the Error type returned in your server's procedures
///
/// # Example
///
/// ```
/// use dcl_rpc::rpc_protocol::{RemoteError, RemoteErrorResponse};
/// pub enum MyEnumOfErrors {
///     EntityNotFound,
///     DbError
/// }
///
/// impl RemoteErrorResponse for MyEnumOfErrors {
///     fn error_code(&self) -> u32 {
///         match self {
///             Self::EntityNotFound => 404,
///             Self::DbError => 500
///         }
///     }
///     
///     fn error_message(&self) -> String {
///         match self {
///             Self::EntityNotFound => "The entity wasn't found".to_string(),
///             Self::DbError => "Internal Server Error".to_string()
///         }
///     }
/// }
///
/// let error: RemoteError = MyEnumOfErrors::EntityNotFound.into();
/// assert_eq!(error.error_code, 404);
/// assert_eq!(error.error_message, "The entity wasn't found")
/// ```
///
///
pub trait RemoteErrorResponse {
    fn error_code(&self) -> u32;
    fn error_message(&self) -> String;
}

/// Every type which implements [`RemoteErrorResponse`], it can be turned into a [`RemoteError`]
impl<T: RemoteErrorResponse> From<T> for RemoteError {
    fn from(value: T) -> Self {
        Self {
            message_identifier: 0, // We cannot know the identifier, it has to be changed after.
            error_code: value.error_code(),
            error_message: value.error_message(),
        }
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
pub(crate) fn fill_remote_error(remote_error: &mut RemoteError, message_number: u32) {
    remote_error.message_identifier = parse::build_message_identifier(
        RpcMessageTypes::RemoteErrorResponse as u32,
        message_number,
    );
}

/// Build the [`ServerReady`](`RpcMessageTypes::ServerReady`) message for the client
pub(crate) fn server_ready_message() -> RpcMessageHeader {
    RpcMessageHeader {
        message_identifier: parse::build_message_identifier(RpcMessageTypes::ServerReady as u32, 0),
    }
}
