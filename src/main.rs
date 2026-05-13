mod backends;
mod cleanup;
mod error;
mod mount;
mod security;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use tracing::{info, warn};
use zeroize::Zeroizing;

use crate::{
    backends::{create_backend, KeySource},
    cleanup::secure_cleanup,
    error::{io_path, BridgeError, Result},
    mount::{mount_and_write_secret, validate_output_path},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendType {
    Age,
    Sops,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BackendArg {
    Auto,
    Age,
    Sops,
}

#[derive(Debug, Parser)]
#[command(name = "nix-secret-bridge", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Decrypt(DecryptArgs),
    DecryptAll(DecryptAllArgs),
    Cleanup(CleanupArgs),
    ValidateMapping(ValidateMappingArgs),
}

#[derive(Debug, Parser)]
struct DecryptArgs {
    #[arg(long, short = 'b', value_enum, default_value_t = BackendArg::Auto)]
    backend: BackendArg,

    #[arg(long, short = 'e')]
    encrypted_file: PathBuf,

    #[arg(long, short = 'o')]
    output_path: PathBuf,

    #[arg(long, short = 'k', env = "NIX_SECRET_BRIDGE_MASTER_KEY_FILE")]
    master_key_file: Option<PathBuf>,

    #[arg(long)]
    interactive: bool,

    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,
}

#[derive(Debug, Parser)]
struct DecryptAllArgs {
    #[arg(long, short = 'm')]
    mapping_file: PathBuf,

    #[arg(long, short = 'k', env = "NIX_SECRET_BRIDGE_MASTER_KEY_FILE")]
    master_key_file: Option<PathBuf>,

    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,
}

#[derive(Debug, Parser)]
struct CleanupArgs {
    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,

    #[arg(long)]
    path: Option<PathBuf>,

    #[arg(long)]
    no_unmount: bool,
}

#[derive(Debug, Parser)]
struct ValidateMappingArgs {
    #[arg(long, short = 'm')]
    mapping_file: PathBuf,

    #[arg(long, default_value = "/run/secrets-bridge")]
    mount_base: PathBuf,

    #[arg(long)]
    allow_missing_inputs: bool,
}

#[derive(Debug, Deserialize)]
struct MappingEntry {
    backend: Option<String>,
    encrypted_file: PathBuf,
    output_path: PathBuf,
}

type SecretMapping = BTreeMap<String, MappingEntry>;

fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    if cli.command.requires_secret_memory_hardening() {
        if let Err(err) = security::harden_process() {
            warn!("process hardening is incomplete: {err}");
        }
    }

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nix-secret-bridge: {err}");
            ExitCode::FAILURE
        }
    }
}

impl Commands {
    fn requires_secret_memory_hardening(&self) -> bool {
        matches!(self, Commands::Decrypt(_) | Commands::DecryptAll(_))
    }
}

fn init_tracing() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .without_time()
        .finish();

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        eprintln!("nix-secret-bridge: failed to initialize tracing subscriber");
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Decrypt(args) => decrypt_one(args),
        Commands::DecryptAll(args) => decrypt_all(args),
        Commands::Cleanup(args) => {
            let selected_path = args
                .path
                .as_deref()
                .map(|path| validate_output_path(&args.mount_base, path))
                .transpose()?;
            secure_cleanup(&args.mount_base, selected_path.as_deref(), !args.no_unmount)
        }
        Commands::ValidateMapping(args) => validate_mapping_command(args),
    }
}

fn decrypt_one(args: DecryptArgs) -> Result<()> {
    let backend_type = detect_backend(args.backend, &args.encrypted_file)?;
    let key_source = resolve_key_source(
        backend_type,
        args.master_key_file.as_deref(),
        args.interactive,
    )?;
    let backend = create_backend(backend_type);

    let plaintext = backend.decrypt(&args.encrypted_file, &key_source)?;
    mount_and_write_secret(&args.mount_base, &args.output_path, &plaintext)?;
    drop(plaintext);

    info!("secret decrypted to {}", args.output_path.display());
    Ok(())
}

fn decrypt_all(args: DecryptAllArgs) -> Result<()> {
    let mapping = read_mapping(&args.mapping_file)?;

    for (name, entry) in mapping {
        let backend_type = backend_from_mapping(entry.backend.as_deref(), &entry.encrypted_file)
            .map_err(|err| BridgeError::Config(format!("secret '{name}': {err}")))?;
        let key_source = resolve_key_source(backend_type, args.master_key_file.as_deref(), false)?;
        let backend = create_backend(backend_type);
        let plaintext = backend.decrypt(&entry.encrypted_file, &key_source)?;
        mount_and_write_secret(&args.mount_base, &entry.output_path, &plaintext)?;
        drop(plaintext);
        info!(
            "secret '{name}' decrypted to {}",
            entry.output_path.display()
        );
    }

    Ok(())
}

fn validate_mapping_command(args: ValidateMappingArgs) -> Result<()> {
    let mapping = read_mapping(&args.mapping_file)?;
    for (name, entry) in mapping {
        backend_from_mapping(entry.backend.as_deref(), &entry.encrypted_file)
            .map_err(|err| BridgeError::Config(format!("secret '{name}': {err}")))?;
        validate_output_path(&args.mount_base, &entry.output_path)
            .map_err(|err| BridgeError::Config(format!("secret '{name}': {err}")))?;

        if !args.allow_missing_inputs && !entry.encrypted_file.exists() {
            return Err(BridgeError::Config(format!(
                "secret '{name}' encrypted file does not exist: '{}'",
                entry.encrypted_file.display()
            )));
        }
    }
    Ok(())
}

fn read_mapping(path: &Path) -> Result<SecretMapping> {
    let text =
        std::fs::read_to_string(path).map_err(|err| io_path("read mapping file", path, err))?;
    serde_json::from_str(&text).map_err(|source| BridgeError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_key_source(
    backend_type: BackendType,
    master_key_file: Option<&Path>,
    interactive: bool,
) -> Result<KeySource> {
    if interactive {
        return Ok(KeySource::Interactive);
    }

    if let Some(path) = master_key_file {
        if !path.exists() {
            return Err(BridgeError::Config(format!(
                "master key file does not exist: '{}'",
                path.display()
            )));
        }
        return Ok(KeySource::File(path.to_path_buf()));
    }

    let env_order: &[&'static str] = match backend_type {
        BackendType::Age => &[
            "NIX_SECRET_BRIDGE_AGE_KEY",
            "AGE_KEY",
            "SOPS_AGE_KEY",
            "SOPS_AGE_KEY_FILE",
        ],
        BackendType::Sops => &[
            "SOPS_AGE_KEY_FILE",
            "SOPS_AGE_KEY",
            "NIX_SECRET_BRIDGE_AGE_KEY",
            "AGE_KEY",
        ],
    };

    for name in env_order {
        if let Some(source) = key_source_from_env(name) {
            return Ok(source);
        }
    }

    Err(BridgeError::Config(
        "no master key source found; set NIX_SECRET_BRIDGE_AGE_KEY, SOPS_AGE_KEY, SOPS_AGE_KEY_FILE, or pass --master-key-file".to_string(),
    ))
}

fn key_source_from_env(name: &'static str) -> Option<KeySource> {
    let value = std::env::var(name).ok()?;
    if value.is_empty() {
        return None;
    }

    if name.ends_with("_KEY_FILE") {
        Some(KeySource::File(PathBuf::from(value)))
    } else {
        Some(KeySource::EnvVar(Zeroizing::new(value)))
    }
}

fn backend_from_mapping(value: Option<&str>, encrypted_file: &Path) -> Result<BackendType> {
    match value {
        Some("age") => Ok(BackendType::Age),
        Some("sops") => Ok(BackendType::Sops),
        Some(other) => Err(BridgeError::Config(format!(
            "unknown backend '{other}', expected 'age' or 'sops'"
        ))),
        None => detect_backend(BackendArg::Auto, encrypted_file),
    }
}

fn detect_backend(selection: BackendArg, encrypted_file: &Path) -> Result<BackendType> {
    match selection {
        BackendArg::Age => Ok(BackendType::Age),
        BackendArg::Sops => Ok(BackendType::Sops),
        BackendArg::Auto => detect_backend_from_path(encrypted_file),
    }
}

fn detect_backend_from_path(path: &Path) -> Result<BackendType> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

    if extension == "age" {
        return Ok(BackendType::Age);
    }

    if file_name.contains(".sops.") || extension == "sops" {
        return Ok(BackendType::Sops);
    }

    Err(BridgeError::Config(format!(
        "could not infer backend from '{}'; pass --backend age or --backend sops",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::{detect_backend_from_path, BackendType};
    use std::path::Path;

    #[test]
    fn detects_age_files() {
        assert_eq!(
            detect_backend_from_path(Path::new("luks-key.age")).expect("age backend"),
            BackendType::Age
        );
    }

    #[test]
    fn detects_sops_files() {
        assert_eq!(
            detect_backend_from_path(Path::new("luks-key.sops.yaml")).expect("sops backend"),
            BackendType::Sops
        );
    }
}
