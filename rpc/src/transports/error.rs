//! Contains the parsing methods for the errors of the current transports.
use std::{io, net::AddrParseError};

use super::TransportError;

impl From<io::Error> for TransportError {
    fn from(value: io::Error) -> Self {
        TransportError::Internal(Box::new(value))
    }
}

impl From<AddrParseError> for TransportError {
    fn from(value: AddrParseError) -> Self {
        TransportError::Internal(Box::new(value))
    }
}
