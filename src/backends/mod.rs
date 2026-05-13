pub mod age;
pub mod sops;

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::{error::BackendError, BackendType};

#[derive(Debug)]
pub enum KeySource {
    EnvVar(Zeroizing<String>),
    File(PathBuf),
    Interactive,
}

pub trait SecretBackend: Send + Sync {
    fn decrypt(
        &self,
        encrypted_path: &Path,
        key_source: &KeySource,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError>;
}

pub fn create_backend(backend_type: BackendType) -> Box<dyn SecretBackend> {
    match backend_type {
        BackendType::Age => Box::new(age::AgeBackend),
        BackendType::Sops => Box::new(sops::SopsBackend),
    }
}
