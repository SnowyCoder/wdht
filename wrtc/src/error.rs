use std::borrow::Cow;

use thiserror::Error;

#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum WrtcError {
    #[error("DataChannel error {0}")]
    DataChannelError(Cow<'static, str>),
    #[error("Connection lost")]
    ConnectionLost,
    #[error("Singaling failed")]
    SignalingFailed,
    #[error("Invalid session description")]
    InvalidDescription,
    #[error("Unknown runtime error: {0}")]
    RuntimeError(String),
}

pub type Result<T> = core::result::Result<T, WrtcError>;
