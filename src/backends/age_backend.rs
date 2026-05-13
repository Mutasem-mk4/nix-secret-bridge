//! age decryption backend using the `rage` crate (pure Rust)

use std::io::Read;
use std::path::Path;
use tracing::{debug, info};
use zeroize::Zeroizing;
use super::{BackendError, KeySource, SecretBackend};

pub struct AgeBackend;

impl AgeBackend {
    pub fn new() -> Self { Self }

    fn parse_identities(key_content: &str) -> Result<Vec<Box<dyn age::Identity>>, BackendError> {
        let identities: Vec<Box<dyn age::Identity>> = age::IdentityFile::from_buffer(key_content.as_bytes())
            .map_err(|e| BackendError::KeySourceError(format!("Failed to parse age identity: {}", e)))?
            .into_identities()
            .into_iter()
            .map(|entry| match entry {
                age::IdentityFileEntry::Native(i) => Box::new(i) as Box<dyn age::Identity>,
                age::IdentityFileEntry::Plugin(i) => Box::new(i) as Box<dyn age::Identity>,
            })
            .collect();
        if identities.is_empty() {
            return Err(BackendError::KeySourceError("No valid age identities found".into()));
        }
        Ok(identities)
    }

    fn decrypt_with_identities(
        encrypted_path: &Path,
        identities: Vec<Box<dyn age::Identity>>,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        let file = std::fs::File::open(encrypted_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                BackendError::FileNotFound(encrypted_path.to_path_buf())
            } else { BackendError::Io(e) }
        })?;
        let decryptor = match age::Decryptor::new(file) {
            Ok(age::Decryptor::Recipients(d)) => d,
            Ok(_) => return Err(BackendError::DecryptionFailed("File is passphrase-encrypted. Use --interactive.".into())),
            Err(e) => return Err(BackendError::DecryptionFailed(format!("Invalid age file: {}", e))),
        };
        let refs: Vec<&dyn age::Identity> = identities.iter().map(|i| i.as_ref()).collect();
        let mut reader = decryptor.decrypt(refs.iter().copied())
            .map_err(|e| BackendError::DecryptionFailed(format!("Decryption failed: {}", e)))?;
        let mut decrypted = Zeroizing::new(Vec::new());
        reader.read_to_end(&mut decrypted)
            .map_err(|e| BackendError::DecryptionFailed(format!("Read error: {}", e)))?;
        Ok(decrypted)
    }
}

impl SecretBackend for AgeBackend {
    fn name(&self) -> &str { "age" }

    fn decrypt(&self, encrypted_path: &Path, key_source: &KeySource) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        debug!(encrypted = %encrypted_path.display(), "Decrypting with age backend");
        match key_source {
            KeySource::EnvVar(key_content) => {
                info!("Using age identity from environment variable");
                let ids = Self::parse_identities(key_content)?;
                Self::decrypt_with_identities(encrypted_path, ids)
            }
            KeySource::File(key_path) => {
                info!(key_file = %key_path.display(), "Using age identity from file");
                if !key_path.exists() {
                    return Err(BackendError::KeySourceError(format!(
                        "Identity file not found: '{}'. Set NIX_SECRET_BRIDGE_AGE_KEY or --master-key-file", key_path.display()
                    )));
                }
                let content = std::fs::read_to_string(key_path)
                    .map_err(|e| BackendError::KeySourceError(format!("Read error: {}", e)))?;
                let ids = Self::parse_identities(&content)?;
                Self::decrypt_with_identities(encrypted_path, ids)
            }
            KeySource::YubiKey => {
                Err(BackendError::NotAvailable("YubiKey support requires age-plugin-yubikey. Provide identity file path.".into()))
            }
            KeySource::Interactive => {
                Err(BackendError::NotAvailable("Interactive passphrase mode not yet implemented for age backend".into()))
            }
        }
    }
}
