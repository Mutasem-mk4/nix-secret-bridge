use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use nix::mount::{umount2, MntFlags};

use crate::error::{io_path, BridgeError, Result};

pub fn secure_cleanup(
    mount_base: &Path,
    selected_path: Option<&Path>,
    unmount: bool,
) -> Result<()> {
    if let Some(path) = selected_path {
        secure_delete_path(path)?;
    } else if mount_base.exists() {
        let files = collect_secret_paths(mount_base)?;
        for path in files {
            secure_delete_path(&path)?;
        }
        remove_empty_dirs(mount_base)?;
    }

    if unmount && mount_base.exists() {
        unmount_mount_base(mount_base)?;

        match fs::remove_dir(mount_base) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) if is_directory_not_empty(&err) => {}
            Err(err) => return Err(io_path("remove tmpfs mount point", mount_base, err)),
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn unmount_mount_base(mount_base: &Path) -> Result<()> {
    match umount2(mount_base, MntFlags::MNT_DETACH) {
        Ok(()) => Ok(()),
        Err(nix::errno::Errno::EINVAL) | Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(err) => Err(BridgeError::Cleanup(format!(
            "failed to unmount '{}': {err}",
            mount_base.display()
        ))),
    }
}

#[cfg(not(target_os = "linux"))]
fn unmount_mount_base(_mount_base: &Path) -> Result<()> {
    Err(BridgeError::UnsupportedPlatform(
        "tmpfs unmounting is only supported on Linux".to_string(),
    ))
}

pub fn secure_delete_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            BridgeError::Cleanup(format!("secret path '{}' does not exist", path.display()))
        } else {
            io_path("inspect secret path", path, err)
        }
    })?;

    if metadata.file_type().is_symlink() {
        fs::remove_file(path).map_err(|err| io_path("unlink symlink secret path", path, err))?;
        return Ok(());
    }

    if !metadata.file_type().is_file() {
        return Err(BridgeError::Cleanup(format!(
            "refusing to securely delete non-regular path '{}'",
            path.display()
        )));
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|err| io_path("open secret for overwrite", path, err))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|err| io_path("seek secret for overwrite", path, err))?;
    write_zeros(&mut file, metadata.len())
        .map_err(|err| io_path("overwrite secret with zeros", path, err))?;
    file.sync_all()
        .map_err(|err| io_path("sync overwritten secret", path, err))?;
    drop(file);

    fs::remove_file(path).map_err(|err| io_path("unlink secret", path, err))?;
    Ok(())
}

fn collect_secret_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    if !dir.exists() {
        return Ok(paths);
    }

    for entry in fs::read_dir(dir).map_err(|err| io_path("read secret directory", dir, err))? {
        let entry = entry.map_err(|err| io_path("read secret directory entry", dir, err))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| io_path("inspect secret path", &path, err))?;

        if metadata.file_type().is_dir() {
            paths.extend(collect_secret_paths(&path)?);
        } else if metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            paths.push(path);
        }
    }

    Ok(paths)
}

fn remove_empty_dirs(dir: &Path) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|err| io_path("read cleanup directory", dir, err))? {
        let entry = entry.map_err(|err| io_path("read cleanup directory entry", dir, err))?;
        let path = entry.path();
        if path.is_dir() {
            remove_empty_dirs(&path)?;
            match fs::remove_dir(&path) {
                Ok(()) => {}
                Err(err) if is_directory_not_empty(&err) => {}
                Err(err) => return Err(io_path("remove empty secret directory", &path, err)),
            }
        }
    }

    Ok(())
}

fn write_zeros(file: &mut fs::File, mut remaining: u64) -> std::io::Result<()> {
    let zeros = [0_u8; 8192];
    while remaining > 0 {
        let n = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..n])?;
        remaining -= n as u64;
    }
    Ok(())
}

fn is_directory_not_empty(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(39) | Some(145))
}
