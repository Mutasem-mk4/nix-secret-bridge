//! SOPS decryption backend — shells out to the `sops` CLI
//!
//! SOPS files can be in JSON, YAML, or binary format. This backend uses the
//! `sops` CLI binary (which must be available in PATH) to decrypt files.
//! The age key is provided via SOPS_AGE_KEY_FILE or a temporary file.

use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};
use zeroize::Zeroizing;
use super::{BackendError, KeySource, SecretBackend};

pub struct SopsBackend;

impl SopsBackend {
    pub fn new() -> Self { Self }

    fn find_sops_binary() -> Result<PathBuf, BackendError> {
        which::which("sops").map_err(|_| {
            BackendError::NotAvailable(
                "sops binary not found in PATH. Install sops or add it to your environment.\n\
                 In NixOS: environment.systemPackages = [ pkgs.sops ];".into()
            )
        })
    }
}

impl SecretBackend for SopsBackend {
    fn name(&self) -> &str { "sops" }

    fn decrypt(
        &self,
        encrypted_path: &Path,
        key_source: &KeySource,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        debug!(encrypted = %encrypted_path.display(), "Decrypting with SOPS backend");

        if !encrypted_path.exists() {
            return Err(BackendError::FileNotFound(encrypted_path.to_path_buf()));
        }

        let sops_bin = Self::find_sops_binary()?;

        let mut cmd = Command::new(&sops_bin);
        cmd.arg("--decrypt");
        cmd.arg("--output-type").arg("binary");
        cmd.arg(encrypted_path);

        // Set the key source for sops
        match key_source {
            KeySource::File(key_path) => {
                info!(key_file = %key_path.display(), "Using SOPS age key file");
                if !key_path.exists() {
                    return Err(BackendError::KeySourceError(format!(
                        "SOPS key file not found: '{}'", key_path.display()
                    )));
                }
                cmd.env("SOPS_AGE_KEY_FILE", key_path);
            }
            KeySource::EnvVar(key_content) => {
                info!("Using SOPS age key from environment");
                // Write to a temporary file for sops (sops needs a file path)
                // Use memfd or tmpfs-backed tempfile to avoid disk writes
                let tmp_dir = std::env::temp_dir();
                let key_file = tmp_dir.join(".nix-secret-bridge-sops-key");
                // In production, use memfd_create or O_TMPFILE
                std::fs::write(&key_file, key_content.as_bytes())
                    .map_err(|e| BackendError::KeySourceError(format!("Failed to write temp key: {}", e)))?;
                cmd.env("SOPS_AGE_KEY_FILE", &key_file);
                // Schedule cleanup (best-effort)
                let _cleanup_guard = scopeguard::guard(key_file.clone(), |p| {
                    let _ = std::fs::remove_file(p);
                });
            }
            KeySource::YubiKey => {
                return Err(BackendError::NotAvailable(
                    "YubiKey not directly supported with SOPS backend".into()
                ));
            }
            KeySource::Interactive => {
                return Err(BackendError::NotAvailable(
                    "Interactive mode not supported with SOPS backend".into()
                ));
            }
        }

        // Suppress stderr to avoid leaking metadata
        cmd.stderr(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());

        let output = cmd.output().map_err(|e| {
            BackendError::DecryptionFailed(format!("Failed to execute sops: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BackendError::DecryptionFailed(format!(
                "sops decryption failed (exit {}): {}", output.status, stderr
            )));
        }

        Ok(Zeroizing::new(output.stdout))
    }
}
