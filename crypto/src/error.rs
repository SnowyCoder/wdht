use thiserror::Error;

#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum CryptoError {
    #[error("Error importing key")]
    ImportKeyError,
    #[error("Unknown internal error: {0}")]
    InternalError(String),
}

pub type Result<T> = core::result::Result<T, CryptoError>;
