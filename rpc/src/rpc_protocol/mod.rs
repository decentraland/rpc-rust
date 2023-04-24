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
