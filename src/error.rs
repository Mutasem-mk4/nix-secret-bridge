//! Custom error types for nix-secret-bridge

use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Backend error: {0}")]
    Backend(#[from] crate::backends::BackendError),

    #[error("Mount error: {0}")]
    Mount(String),

    #[error("Cleanup error: {0}")]
    Cleanup(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
