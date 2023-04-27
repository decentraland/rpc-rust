//! Contains the parsing methods for the errors of the current transports.
use std::{io, net::AddrParseError};

#[cfg(feature = "quic")]
use quinn::{ConnectError, ConnectionError, WriteError};
#[cfg(feature = "websockets")]
use tokio_tungstenite::tungstenite;

use super::TransportError;

impl From<io::Error> for TransportError {
    fn from(value: io::Error) -> Self {
        TransportError::Internal(Box::new(value))
    }
}

#[cfg(feature = "quic")]
impl From<ConnectError> for TransportError {
    fn from(value: ConnectError) -> Self {
        TransportError::Internal(Box::new(value))
    }
}

#[cfg(feature = "quic")]
impl From<ConnectionError> for TransportError {
    fn from(value: ConnectionError) -> Self {
        TransportError::Internal(Box::new(value))
    }
}

impl From<AddrParseError> for TransportError {
    fn from(value: AddrParseError) -> Self {
        TransportError::Internal(Box::new(value))
    }
}

#[cfg(feature = "quic")]
impl From<WriteError> for TransportError {
    fn from(value: WriteError) -> Self {
        match value {
            WriteError::ZeroRttRejected => {
                TransportError::Internal(Box::new(WriteError::ZeroRttRejected))
            }
            _ => TransportError::Closed,
        }
    }
}

#[cfg(feature = "websockets")]
impl From<tungstenite::Error> for TransportError {
    fn from(value: tungstenite::Error) -> Self {
        TransportError::Internal(Box::new(value))
    }
}
