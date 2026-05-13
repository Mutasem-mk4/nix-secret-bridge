use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use zeroize::Zeroizing;

use crate::{
    backends::{KeySource, SecretBackend},
    error::BackendError,
};

pub struct SopsBackend;

impl SopsBackend {
    fn sops_binary() -> Result<PathBuf, BackendError> {
        which::which("sops").map_err(|_| {
            BackendError::NotAvailable(
                "the sops binary was not found in PATH; add pkgs.sops to the installer environment"
                    .to_string(),
            )
        })
    }

    fn stderr_summary(stderr: &[u8]) -> String {
        let text = String::from_utf8_lossy(stderr);
        let text = text.trim();
        if text.is_empty() {
            "sops exited with a non-zero status and no stderr".to_string()
        } else {
            text.chars().take(4096).collect()
        }
    }
}

impl SecretBackend for SopsBackend {
    fn decrypt(
        &self,
        encrypted_path: &Path,
        key_source: &KeySource,
    ) -> Result<Zeroizing<Vec<u8>>, BackendError> {
        if !encrypted_path.exists() {
            return Err(BackendError::FileNotFound(encrypted_path.to_path_buf()));
        }

        let mut command = Command::new(Self::sops_binary()?);
        command
            .arg("--decrypt")
            .arg("--output-type")
            .arg("binary")
            .arg(encrypted_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match key_source {
            KeySource::EnvVar(value) => {
                command.env("SOPS_AGE_KEY", value.as_str());
                command.env_remove("SOPS_AGE_KEY_FILE");
            }
            KeySource::File(path) => {
                if !path.exists() {
                    return Err(BackendError::KeySource(format!(
                        "SOPS age key file does not exist: '{}'",
                        path.display()
                    )));
                }
                command.env("SOPS_AGE_KEY_FILE", path);
            }
            KeySource::Interactive => {
                return Err(BackendError::NotAvailable(
                    "interactive SOPS decryption is not supported in unattended installer runs"
                        .to_string(),
                ));
            }
        }

        let output = command
            .output()
            .map_err(|err| BackendError::Decryption(format!("failed to execute sops: {err}")))?;

        if !output.status.success() {
            return Err(BackendError::Decryption(format!(
                "sops failed with status {}: {}",
                output.status,
                Self::stderr_summary(&output.stderr)
            )));
        }

        Ok(Zeroizing::new(output.stdout))
    }
}
