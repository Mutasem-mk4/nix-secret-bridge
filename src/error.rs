use std::{io, path::PathBuf};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, BridgeError>;

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error(transparent)]
    Backend(#[from] BackendError),

    #[error("mount error: {0}")]
    Mount(String),

    #[error("cleanup error: {0}")]
    Cleanup(String),

    #[error("failed to {action} '{}': {source}", path.display())]
    IoPath {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse JSON mapping '{}': {source}", path.display())]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[cfg_attr(target_os = "linux", allow(dead_code))]
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("encrypted file not found: '{}'", .0.display())]
    FileNotFound(PathBuf),

    #[error("key source error: {0}")]
    KeySource(String),

    #[error("decryption failed: {0}")]
    Decryption(String),

    #[error("backend is not available: {0}")]
    NotAvailable(String),

    #[error("failed to {action} '{}': {source}", path.display())]
    IoPath {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub fn io_path(action: &'static str, path: impl Into<PathBuf>, source: io::Error) -> BridgeError {
    BridgeError::IoPath {
        action,
        path: path.into(),
        source,
    }
}

pub fn backend_io_path(
    action: &'static str,
    path: impl Into<PathBuf>,
    source: io::Error,
) -> BackendError {
    BackendError::IoPath {
        action,
        path: path.into(),
        source,
    }
}
