//! nix-secret-bridge: Bootstrap secret orchestrator for NixOS
//!
//! This crate provides the CLI entry point for decrypting secrets during
//! the disk partitioning phase of a NixOS deployment, before the target
//! system is booted. It bridges the gap between encrypted secret files
//! (age, SOPS) and tools like `disko` that need plaintext keys at format time.
//!
//! # Security Properties
//!
//! - Decrypted secrets exist only in memory and on tmpfs (never persistent storage)
//! - All in-memory secret buffers are zeroized on drop via the `zeroize` crate
//! - Process memory is locked (mlock) to prevent swapping
//! - Core dumps are disabled via prctl(PR_SET_DUMPABLE, 0)
//! - No secret data is ever logged

mod backends;
mod cleanup;
mod error;
mod mount;
mod security;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use tracing::{info, warn};

use crate::backends::{create_backend, KeySource};
use crate::cleanup::secure_cleanup;
use crate::mount::mount_and_write_secret;
use crate::security::harden_process;

/// nix-secret-bridge — Bootstrap secret orchestrator for NixOS
///
/// Decrypts secrets during disk partitioning (before system boot) and exposes
/// them on a tmpfs mount point for consumption by disko's luksFormat.
#[derive(Parser, Debug)]
#[command(
    name = "nix-secret-bridge",
    version,
    about = "Decrypt secrets during NixOS disk partitioning",
    long_about = None,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Decrypt a secret and expose it on tmpfs
    Decrypt(DecryptArgs),

    /// Securely clean up all decrypted secrets
    Cleanup(CleanupArgs),

    /// Decrypt multiple secrets from a JSON mapping file
    DecryptAll(DecryptAllArgs),
}

#[derive(Parser, Debug)]
struct DecryptArgs {
    /// Decryption backend to use
    #[arg(long, short = 'b', value_enum)]
    backend: BackendType,

    /// Path to the encrypted secret file
    #[arg(long, short = 'e')]
    encrypted_file: PathBuf,

    /// Output path on tmpfs (e.g., /run/secrets-bridge/luks-key)
    #[arg(long, short = 'o')]
    output_path: PathBuf,

    /// Path to the master key file (alternative to environment variable)
    #[arg(long, short = 'k', env = "NIX_SECRET_BRIDGE_MASTER_KEY_FILE")]
    master_key_file: Option<PathBuf>,

    /// Use YubiKey for decryption (age-plugin-yubikey)
    #[arg(long)]
    yubikey: bool,

    /// Prompt for passphrase interactively
    #[arg(long)]
    interactive: bool,

    /// Base directory for the tmpfs mount
    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,
}

#[derive(Parser, Debug)]
struct CleanupArgs {
    /// Base directory for the tmpfs mount to clean up
    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,
}

#[derive(Parser, Debug)]
struct DecryptAllArgs {
    /// Path to a JSON file containing the secret mapping
    /// Format: { "secret-name": { "backend": "age", "encrypted_file": "path", "output_path": "path" } }
    #[arg(long, short = 'm')]
    mapping_file: PathBuf,

    /// Path to the master key file (alternative to environment variable)
    #[arg(long, short = 'k', env = "NIX_SECRET_BRIDGE_MASTER_KEY_FILE")]
    master_key_file: Option<PathBuf>,

    /// Base directory for the tmpfs mount
    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum BackendType {
    /// age encryption (compatible with agenix)
    Age,
    /// SOPS encryption (compatible with sops-nix)
    Sops,
}

fn main() -> Result<()> {
    // Initialize tracing with sanitized output (never log secrets)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // Harden the process: disable core dumps, lock memory
    if let Err(e) = harden_process() {
        warn!("Failed to fully harden process (non-fatal): {}", e);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Decrypt(args) => cmd_decrypt(args),
        Commands::Cleanup(args) => cmd_cleanup(args),
        Commands::DecryptAll(args) => cmd_decrypt_all(args),
    }
}

/// Execute the `decrypt` subcommand: decrypt a single secret and place it on tmpfs
fn cmd_decrypt(args: DecryptArgs) -> Result<()> {
    info!(
        backend = %format!("{:?}", args.backend),
        encrypted = %args.encrypted_file.display(),
        output = %args.output_path.display(),
        "Decrypting secret"
    );

    // Determine the key source
    let key_source = resolve_key_source(
        args.master_key_file.as_deref(),
        args.yubikey,
        args.interactive,
        &args.backend,
    )?;

    // Create the appropriate backend
    let backend = create_backend(&args.backend)?;

    // Decrypt the secret — result is Zeroizing<Vec<u8>>, auto-zeroized on drop
    let decrypted = backend
        .decrypt(&args.encrypted_file, &key_source)
        .with_context(|| {
            format!(
                "Failed to decrypt '{}' with {:?} backend",
                args.encrypted_file.display(),
                args.backend,
            )
        })?;

    // Mount tmpfs and write the decrypted secret
    mount_and_write_secret(&args.mount_base, &args.output_path, &decrypted)
        .with_context(|| {
            format!(
                "Failed to write decrypted secret to '{}'",
                args.output_path.display()
            )
        })?;

    // decrypted is dropped here → zeroized automatically
    info!(
        output = %args.output_path.display(),
        "Secret successfully decrypted and placed on tmpfs"
    );

    Ok(())
}

/// Execute the `cleanup` subcommand: securely remove all secrets from tmpfs
fn cmd_cleanup(args: CleanupArgs) -> Result<()> {
    info!(
        mount_base = %args.mount_base.display(),
        "Cleaning up decrypted secrets"
    );

    secure_cleanup(&args.mount_base).with_context(|| {
        format!(
            "Failed to clean up secrets at '{}'",
            args.mount_base.display()
        )
    })?;

    info!("All secrets securely cleaned up");
    Ok(())
}

/// Execute the `decrypt-all` subcommand: process a JSON mapping of multiple secrets
fn cmd_decrypt_all(args: DecryptAllArgs) -> Result<()> {
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize)]
    struct SecretEntry {
        backend: String,
        encrypted_file: PathBuf,
        output_path: PathBuf,
    }

    let mapping_content = std::fs::read_to_string(&args.mapping_file)
        .with_context(|| {
            format!(
                "Failed to read mapping file '{}'",
                args.mapping_file.display()
            )
        })?;

    let mapping: HashMap<String, SecretEntry> =
        serde_json::from_str(&mapping_content).with_context(|| {
            format!(
                "Failed to parse mapping file '{}' as JSON",
                args.mapping_file.display()
            )
        })?;

    info!(count = mapping.len(), "Processing secret mapping");

    for (name, entry) in &mapping {
        let backend_type = match entry.backend.as_str() {
            "age" => BackendType::Age,
            "sops" => BackendType::Sops,
            other => {
                anyhow::bail!(
                    "Unknown backend '{}' for secret '{}'. Supported: age, sops",
                    other,
                    name
                );
            }
        };

        let key_source = resolve_key_source(
            args.master_key_file.as_deref(),
            false,
            false,
            &backend_type,
        )?;

        let backend = create_backend(&backend_type)?;

        let decrypted = backend
            .decrypt(&entry.encrypted_file, &key_source)
            .with_context(|| {
                format!(
                    "Failed to decrypt secret '{}' from '{}'",
                    name,
                    entry.encrypted_file.display()
                )
            })?;

        mount_and_write_secret(&args.mount_base, &entry.output_path, &decrypted)
            .with_context(|| {
                format!(
                    "Failed to write secret '{}' to '{}'",
                    name,
                    entry.output_path.display()
                )
            })?;

        info!(name = %name, output = %entry.output_path.display(), "Secret decrypted");
    }

    info!(count = mapping.len(), "All secrets successfully decrypted");
    Ok(())
}

/// Resolve the key source from CLI arguments, environment, or prompt
fn resolve_key_source(
    master_key_file: Option<&std::path::Path>,
    yubikey: bool,
    interactive: bool,
    backend: &BackendType,
) -> Result<KeySource> {
    if yubikey {
        return Ok(KeySource::YubiKey);
    }

    if interactive {
        return Ok(KeySource::Interactive);
    }

    if let Some(path) = master_key_file {
        if !path.exists() {
            anyhow::bail!(
                "Master key file '{}' does not exist.\n\
                 Hint: Provide a valid key file via --master-key-file or set \
                 the NIX_SECRET_BRIDGE_MASTER_KEY_FILE environment variable.",
                path.display()
            );
        }
        return Ok(KeySource::File(path.to_path_buf()));
    }

    // Try environment variables based on backend
    match backend {
        BackendType::Age => {
            if let Ok(key) = std::env::var("NIX_SECRET_BRIDGE_AGE_KEY") {
                if !key.is_empty() {
                    return Ok(KeySource::EnvVar(key));
                }
            }
            // Fallback to standard age key location
            if let Ok(path) = std::env::var("SOPS_AGE_KEY_FILE") {
                return Ok(KeySource::File(PathBuf::from(path)));
            }
        }
        BackendType::Sops => {
            if let Ok(path) = std::env::var("SOPS_AGE_KEY_FILE") {
                return Ok(KeySource::File(PathBuf::from(path)));
            }
            if let Ok(key) = std::env::var("NIX_SECRET_BRIDGE_AGE_KEY") {
                if !key.is_empty() {
                    return Ok(KeySource::EnvVar(key));
                }
            }
        }
    }

    anyhow::bail!(
        "No master key source found.\n\
         \n\
         Please provide a key using one of the following methods:\n\
         \n\
         1. Set the NIX_SECRET_BRIDGE_AGE_KEY environment variable:\n\
         \x20  export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)\n\
         \n\
         2. Provide a key file:\n\
         \x20  --master-key-file /path/to/age-identity.txt\n\
         \n\
         3. Use a YubiKey:\n\
         \x20  --yubikey\n\
         \n\
         4. Enter passphrase interactively:\n\
         \x20  --interactive"
    )
}
