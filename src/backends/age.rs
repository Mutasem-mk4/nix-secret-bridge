use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
};

use zeroize::Zeroizing;

use crate::{
    backends::{KeySource, SecretBackend},
    error::{backend_io_path, BackendError},
};

pub struct AgeBackend;

impl AgeBackend {
    fn parse_identities(identity_text: &str) -> Result<Vec<Box<dyn age::Identity>>, BackendError> {
        let identity_file = age::IdentityFile::from_buffer(BufReader::new(Cursor::new(
            identity_text.as_bytes(),
        )))
        .map_err(|err| BackendError::KeySource(format!("invalid age identity file: {err}")))?;

        let identities = identity_file
            .into_identities()
            .map_err(|err| BackendError::KeySource(format!("invalid age identity: {err}")))?;

        if identities.is_empty() {
            return Err(BackendError::KeySource(
                "age identity file contains no identities".to_string(),
            ));
        }

        Ok(identities)
    }

    fn identities_from_file(path: &Path) -> Result<Vec<Box<dyn age::Identity>>, BackendError> {
        let identity_text = std::fs::read_to_string(path)
            .map_err(|err| backend_io_path("read age identity file", path, err))?;
        Self::parse_identities(&identity_text)
    }

    fn decrypt_with_identities(
        encrypted_path: &Path,
        identities: &[Box<dyn age::Identity>],
    ) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        let input = File::open(encrypted_path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                BackendError::FileNotFound(encrypted_path.to_path_buf())
            } else {
                backend_io_path("open encrypted age file", encrypted_path, err)
            }
        })?;

        let armored_or_binary = age::armor::ArmoredReader::new(BufReader::new(input));
        let decryptor = age::Decryptor::new_buffered(armored_or_binary)
            .map_err(|err| BackendError::Decryption(format!("invalid age file: {err}")))?;

        if decryptor.is_scrypt() {
            return Err(BackendError::NotAvailable(
                "passphrase-encrypted age files are not supported in non-interactive installer runs"
                    .to_string(),
            ));
        }

        let mut reader = decryptor
            .decrypt(identities.iter().map(|identity| identity.as_ref()))
            .map_err(|err| BackendError::Decryption(format!("no matching age identity: {err}")))?;

        let mut plaintext = Zeroizing::new(Vec::new());
        reader
            .read_to_end(&mut plaintext)
            .map_err(|err| BackendError::Decryption(format!("failed to read plaintext: {err}")))?;

        Ok(plaintext)
    }
}

impl SecretBackend for AgeBackend {
    fn decrypt(
        &self,
        encrypted_path: &Path,
        key_source: &KeySource,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        let identities = match key_source {
            KeySource::EnvVar(value) => Self::parse_identities(value.as_str())?,
            KeySource::File(path) => Self::identities_from_file(path)?,
            KeySource::Interactive => {
                return Err(BackendError::NotAvailable(
                    "interactive age decryption is intentionally disabled for unattended disko runs"
                        .to_string(),
                ));
            }
        };

        Self::decrypt_with_identities(encrypted_path, &identities)
    }
}
