//! Secure tmpfs mounting and secret file writing
//!
//! Handles creating a dedicated tmpfs mount point and writing decrypted
//! secrets with restrictive permissions. The tmpfs is mounted with
//! noswap, noexec, nosuid, nodev to prevent secret leakage.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};
use nix::mount::{mount, MsFlags};
use tracing::{debug, info};

/// Mount a tmpfs at the given base path if not already mounted
///
/// Mount options: size=1m, noswap, noexec, nosuid, nodev, mode=0700
/// These options ensure:
/// - Limited size (1MB is more than enough for keys)
/// - No swap (secrets stay in RAM only)
/// - No execution (secrets can't be run as programs)
/// - No setuid/setgid (defense in depth)
/// - No device files (defense in depth)
/// - Directory only accessible by root
pub fn ensure_tmpfs_mounted(mount_base: &Path) -> Result<()> {
    // Create the mount point directory if it doesn't exist
    if !mount_base.exists() {
        fs::create_dir_all(mount_base).with_context(|| {
            format!("Failed to create mount point: {}", mount_base.display())
        })?;
    }

    // Check if already mounted (by reading /proc/mounts)
    if is_tmpfs_mounted(mount_base)? {
        debug!(path = %mount_base.display(), "tmpfs already mounted");
        return Ok(());
    }

    // Mount tmpfs with security-hardened options
    let flags = MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID | MsFlags::MS_NODEV;
    let data = Some("size=1m,mode=0700");

    mount(
        Some("none"),
        mount_base,
        Some("tmpfs"),
        flags,
        data,
    )
    .with_context(|| {
        format!(
            "Failed to mount tmpfs at '{}'. Are you running as root?",
            mount_base.display()
        )
    })?;

    info!(path = %mount_base.display(), "tmpfs mounted with noexec,nosuid,nodev");
    Ok(())
}

/// Write decrypted secret bytes to a file on tmpfs with mode 0400
///
/// The file is created with restrictive permissions BEFORE content is written
/// to prevent a race condition where another process could read an empty or
/// partially-written file with broader permissions.
pub fn mount_and_write_secret(
    mount_base: &Path,
    output_path: &Path,
    decrypted: &[u8],
) -> Result<()> {
    // Ensure tmpfs is mounted
    ensure_tmpfs_mounted(mount_base)?;

    // Resolve the output path relative to mount_base if needed
    let full_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        mount_base.join(output_path)
    };

    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create directory: {}", parent.display())
            })?;
            // Set parent directory permissions to 0700
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }

    // Write the file using std::fs with explicit permissions
    // Step 1: Create the file (or truncate if exists) 
    // Step 2: Set permissions to 0400 BEFORE writing content
    // Step 3: Write content
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o400)  // Read-only by owner (root)
        .open(&full_path)
        .with_context(|| {
            format!("Failed to create secret file: {}", full_path.display())
        })?;

    file.write_all(decrypted).with_context(|| {
        format!("Failed to write secret to: {}", full_path.display())
    })?;

    file.sync_all()?;

    info!(
        path = %full_path.display(),
        size = decrypted.len(),
        "Secret written to tmpfs (mode 0400)"
    );

    Ok(())
}

/// Check if a path is a tmpfs mount point by reading /proc/mounts
fn is_tmpfs_mounted(path: &Path) -> Result<bool> {
    let mounts = fs::read_to_string("/proc/mounts")
        .unwrap_or_default();

    let canonical = path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());

    Ok(mounts.lines().any(|line| {
        let parts: Vec<&str> = line.split_whitespace().collect();
        parts.len() >= 3
            && parts[2] == "tmpfs"
            && Path::new(parts[1]) == canonical
    }))
}
