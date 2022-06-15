use std::borrow::Cow;

use thiserror::Error;
use wdht_logic::transport::TransportError;
use wdht_wrtc::WrtcError;

#[derive(Error, Debug, Clone)]
pub enum WrtcTransportError {
    #[error("{0}")]
    WrtcError(wdht_wrtc::WrtcError),
    #[error("Invalid message received")]
    InvalidMessage,
    #[error("Connection limit reached")]
    ConnectionLimitReached,
    #[error("Already connecting to that id")]
    AlreadyConnecting,
    #[error("Transport error: {0}")]
    Transport(TransportError),
    #[error("Unknown error: {0}")]
    UnknownError(Cow<'static, str>),
}

impl From<WrtcError> for WrtcTransportError {
    fn from(x: WrtcError) -> Self {
        WrtcTransportError::WrtcError(x)
    }
}

impl From<TransportError> for WrtcTransportError {
    fn from(x: TransportError) -> Self {
        WrtcTransportError::Transport(x)
    }
}

impl From<&'static str> for WrtcTransportError {
    fn from(x: &'static str) -> Self {
        WrtcTransportError::UnknownError(x.into())
    }
}

impl From<String> for WrtcTransportError {
    fn from(x: String) -> Self {
        WrtcTransportError::UnknownError(x.into())
    }
}
