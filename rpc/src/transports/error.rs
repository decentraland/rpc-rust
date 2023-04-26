//! Contains the parsing methods for the errors of the current transports.
use std::{io, net::AddrParseError};

#[cfg(feature = "quic")]
use quinn::{ConnectError, ConnectionError, WriteError};
#[cfg(feature = "websockets")]
use tokio_tungstenite::tungstenite;

use super::TransportError;

impl From<io::Error> for TransportError {
    fn from(_value: io::Error) -> Self {
        TransportError::Internal
    }
}

#[cfg(feature = "quic")]
impl From<ConnectError> for TransportError {
    fn from(_value: ConnectError) -> Self {
        TransportError::Internal
    }
}

#[cfg(feature = "quic")]
impl From<ConnectionError> for TransportError {
    fn from(_value: ConnectionError) -> Self {
        TransportError::Internal
    }
}

impl From<AddrParseError> for TransportError {
    fn from(_value: AddrParseError) -> Self {
        TransportError::Internal
    }
}

#[cfg(feature = "quic")]
impl From<WriteError> for TransportError {
    fn from(_value: WriteError) -> Self {
        TransportError::Connection
    }
}

#[cfg(feature = "websockets")]
impl From<tungstenite::Error> for TransportError {
    fn from(_value: tungstenite::Error) -> Self {
        TransportError::Internal
    }
}
