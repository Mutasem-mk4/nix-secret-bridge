//! Secure cleanup: zeroize files, unlink, and unmount tmpfs

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use nix::mount::{umount2, MntFlags};
use tracing::{debug, info, warn};

/// Securely clean up all secrets from the tmpfs mount
///
/// 1. Walk all files under the mount base
/// 2. Overwrite each file's contents with zeros
/// 3. Unlink (delete) each file
/// 4. Unmount the tmpfs
pub fn secure_cleanup(mount_base: &Path) -> Result<()> {
    if !mount_base.exists() {
        info!(path = %mount_base.display(), "Mount base does not exist, nothing to clean");
        return Ok(());
    }

    // Step 1: Find and securely delete all files
    let files = collect_files(mount_base)?;
    info!(count = files.len(), "Found secret files to clean up");

    for file_path in &files {
        if let Err(e) = secure_delete_file(file_path) {
            warn!(
                path = %file_path.display(),
                error = %e,
                "Failed to securely delete file (continuing cleanup)"
            );
        }
    }

    // Step 2: Remove empty directories
    remove_empty_dirs(mount_base)?;

    // Step 3: Unmount the tmpfs
    match umount2(mount_base, MntFlags::MNT_DETACH) {
        Ok(()) => info!(path = %mount_base.display(), "tmpfs unmounted"),
        Err(e) => {
            warn!(
                path = %mount_base.display(),
                error = %e,
                "Failed to unmount tmpfs (may not have been mounted)"
            );
        }
    }

    // Step 4: Remove the mount point directory
    if mount_base.exists() {
        let _ = fs::remove_dir(mount_base);
    }

    info!("Secure cleanup complete");
    Ok(())
}

/// Overwrite a file with zeros, then unlink it
fn secure_delete_file(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Failed to stat: {}", path.display()))?;

    let size = metadata.len() as usize;

    // Overwrite with zeros
    if size > 0 {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open for overwrite: {}", path.display()))?;

        let zeros = vec![0u8; size];
        file.write_all(&zeros)?;
        file.sync_all()?;

        debug!(path = %path.display(), size, "File overwritten with zeros");
    }

    // Unlink the file
    fs::remove_file(path)
        .with_context(|| format!("Failed to unlink: {}", path.display()))?;

    info!(path = %path.display(), "File securely deleted");
    Ok(())
}

/// Recursively collect all regular files under a directory
fn collect_files(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files(&path)?);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Remove empty directories under the mount base (bottom-up)
fn remove_empty_dirs(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                remove_empty_dirs(&entry.path())?;
                let _ = fs::remove_dir(entry.path());
            }
        }
    }
    Ok(())
}
