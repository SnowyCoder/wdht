use std::borrow::Cow;

use thiserror::Error;
use wdht_logic::{transport::TransportError, Id};
use wdht_wrtc::WrtcError;

#[derive(Clone, Debug, Error)]
#[non_exhaustive]
pub enum HandshakeError {
    #[error("Received message with bad format")]
    BadFormat,

    #[error("Provided identity is invalid")]
    InvalidIdentity,

    #[error("Connection lost")]
    ConnectionLost,

    #[error("Channel opened")]
    OpenedChannel,

    #[error("A channel with the same ID was already open")]
    IdConflict(Id),

    #[error("WebRTC error: {0}")]
    Wrtc(wdht_wrtc::WrtcError),

    #[error("Internal error: {0}")]
    Internal(&'static str),
}

impl From<WrtcError> for HandshakeError {
    fn from(e: WrtcError) -> Self {
        HandshakeError::Wrtc(e)
    }
}

impl From<serde_json::Error> for HandshakeError {
    fn from(_: serde_json::Error) -> Self {
        HandshakeError::BadFormat
    }
}

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
    #[error("Error occurred during handshake: {0}")]
    Handshake(HandshakeError),
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

impl From<HandshakeError> for WrtcTransportError {
    fn from(x: HandshakeError) -> Self {
        WrtcTransportError::Handshake(x)
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
