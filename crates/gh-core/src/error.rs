use thiserror::Error;

#[derive(Debug, Error)]
pub enum GhCoreError {
    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Octocrab error: {0}")]
    Octocrab(#[from] octocrab::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Hex decoding error: {0}")]
    Hex(#[from] hex::FromHexError),

    #[error("Invalid HMAC key length")]
    HmacInvalidKeyLength,

    #[error("Missing expected header: {0}")]
    MissingHeader(&'static str),

    #[error("Invalid header format: {0}")]
    InvalidHeader(String),

    #[error("Invalid webhook signature")]
    InvalidSignature,

    #[error("Installation token exchange failed with status {status}: {message}")]
    TokenExchangeFailed { status: u16, message: String },

    #[error("Configuration error: {0}")]
    Config(String),
}
