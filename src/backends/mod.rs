//! Backend trait and registry for secret decryption
//!
//! This module defines the `SecretBackend` trait that all decryption backends
//! must implement, and provides a factory function to create backend instances.

pub mod age_backend;
pub mod sops_backend;

use std::path::{Path, PathBuf};

use anyhow::Result;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::BackendType;

/// Errors specific to backend operations
#[derive(Error, Debug)]
pub enum BackendError {
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Key source error: {0}")]
    KeySourceError(String),

    #[error("Encrypted file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Backend not available: {0}")]
    NotAvailable(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// How the master key is provided to the backend
#[derive(Debug, Clone)]
pub enum KeySource {
    /// Key content passed directly (e.g., from environment variable)
    EnvVar(String),

    /// Path to a file containing the key identity
    File(PathBuf),

    /// Use a YubiKey via age-plugin-yubikey
    YubiKey,

    /// Prompt the user interactively for a passphrase
    Interactive,
}

/// Trait that all decryption backends must implement
///
/// Implementors must ensure:
/// - Decrypted data is returned in a `Zeroizing<Vec<u8>>` wrapper
/// - No decrypted data is logged or written to disk
/// - Errors do not contain decrypted content
pub trait SecretBackend: Send + Sync {
    /// Human-readable name for error messages and logging
    fn name(&self) -> &str;

    /// Decrypt an encrypted file, returning the raw decrypted bytes.
    ///
    /// The returned buffer is wrapped in `Zeroizing` to ensure automatic
    /// zeroization when dropped. The caller should consume the bytes as
    /// quickly as possible and let the buffer drop.
    ///
    /// # Errors
    ///
    /// Returns `BackendError` if:
    /// - The encrypted file cannot be read
    /// - The key source is invalid or unavailable
    /// - Decryption fails (wrong key, corrupted file, etc.)
    fn decrypt(
        &self,
        encrypted_path: &Path,
        key_source: &KeySource,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError>;
}

/// Create a backend instance from the backend type
pub fn create_backend(backend_type: &BackendType) -> Result<Box<dyn SecretBackend>> {
    match backend_type {
        BackendType::Age => Ok(Box::new(age_backend::AgeBackend::new())),
        BackendType::Sops => Ok(Box::new(sops_backend::SopsBackend::new())),
    }
}
