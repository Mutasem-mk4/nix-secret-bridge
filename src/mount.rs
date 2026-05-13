use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(target_os = "linux")]
use nix::mount::{mount, MsFlags};
use tempfile::Builder;

use crate::error::{io_path, BridgeError, Result};

#[cfg(target_os = "linux")]
const TMPFS_OPTIONS_WITH_NOSWAP: &str = "size=1m,mode=0700,noswap";
#[cfg(target_os = "linux")]
const TMPFS_OPTIONS_FALLBACK: &str = "size=1m,mode=0700";

pub fn validate_output_path(mount_base: &Path, output_path: &Path) -> Result<PathBuf> {
    if has_parent_component(mount_base) || has_parent_component(output_path) {
        return Err(BridgeError::Mount(
            "mount base and output path must not contain '..' components".to_string(),
        ));
    }

    let full_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        mount_base.join(output_path)
    };

    if !full_path.starts_with(mount_base) {
        return Err(BridgeError::Mount(format!(
            "refusing to write '{}' outside tmpfs mount '{}'",
            full_path.display(),
            mount_base.display()
        )));
    }

    if full_path == mount_base {
        return Err(BridgeError::Mount(
            "output path must refer to a file below the tmpfs mount".to_string(),
        ));
    }

    Ok(full_path)
}

pub fn ensure_tmpfs_mounted(mount_base: &Path) -> Result<()> {
    fs::create_dir_all(mount_base)
        .map_err(|err| io_path("create tmpfs mount point", mount_base, err))?;
    set_mode(mount_base, 0o700, "set mount point permissions")?;

    #[cfg(target_os = "linux")]
    {
        if let Some(fs_type) = mounted_fs_type(mount_base)? {
            if fs_type == "tmpfs" {
                return Ok(());
            }

            return Err(BridgeError::Mount(format!(
                "'{}' is already mounted as {}, not tmpfs",
                mount_base.display(),
                fs_type
            )));
        }

        let flags = MsFlags::MS_NODEV | MsFlags::MS_NOEXEC | MsFlags::MS_NOSUID;
        match mount(
        Some("tmpfs"),
        mount_base,
        Some("tmpfs"),
        flags,
        Some(TMPFS_OPTIONS_WITH_NOSWAP),
    ) {
        Ok(()) => Ok(()),
        Err(first_err) => mount(
            Some("tmpfs"),
            mount_base,
            Some("tmpfs"),
            flags,
            Some(TMPFS_OPTIONS_FALLBACK),
        )
        .map_err(|second_err| {
            BridgeError::Mount(format!(
                "failed to mount tmpfs at '{}': {first_err}; fallback without noswap also failed: {second_err}",
                mount_base.display()
            ))
            }),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(BridgeError::UnsupportedPlatform(
            "tmpfs mounting is only supported on Linux".to_string(),
        ))
    }
}

pub fn mount_and_write_secret(
    mount_base: &Path,
    output_path: &Path,
    plaintext: &[u8],
) -> Result<()> {
    ensure_tmpfs_mounted(mount_base)?;
    let full_path = validate_output_path(mount_base, output_path)?;

    let parent = full_path.parent().ok_or_else(|| {
        BridgeError::Mount(format!(
            "output path '{}' has no parent directory",
            full_path.display()
        ))
    })?;

    fs::create_dir_all(parent).map_err(|err| io_path("create secret directory", parent, err))?;
    set_mode(parent, 0o700, "set secret directory permissions")?;

    if full_path.exists() {
        zero_existing_regular_file(&full_path)?;
    }

    let mut temp = Builder::new()
        .prefix(".nix-secret-bridge.")
        .tempfile_in(parent)
        .map_err(|err| io_path("create temporary secret file", parent, err))?;

    temp.as_file_mut()
        .write_all(plaintext)
        .map_err(|err| io_path("write temporary secret file", temp.path(), err))?;
    temp.as_file_mut()
        .sync_all()
        .map_err(|err| io_path("sync temporary secret file", temp.path(), err))?;

    set_mode(temp.path(), 0o400, "set temporary secret permissions")?;

    temp.persist(&full_path)
        .map_err(|err| io_path("persist secret file", &full_path, err.error))?;
    set_mode(&full_path, 0o400, "set secret file permissions")?;

    Ok(())
}

fn zero_existing_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| io_path("inspect existing secret file", path, err))?;
    if !metadata.file_type().is_file() {
        return Err(BridgeError::Mount(format!(
            "refusing to replace non-regular output path '{}'",
            path.display()
        )));
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|err| io_path("open existing secret file", path, err))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|err| io_path("seek existing secret file", path, err))?;

    write_zeros(&mut file, metadata.len())
        .map_err(|err| io_path("overwrite existing secret file", path, err))?;
    file.sync_all()
        .map_err(|err| io_path("sync existing secret file", path, err))?;
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

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32, action: &'static str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|err| io_path(action, path, err))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32, _action: &'static str) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn mounted_fs_type(path: &Path) -> Result<Option<String>> {
    let mounts = fs::read_to_string("/proc/mounts")
        .map_err(|err| io_path("read /proc/mounts", "/proc/mounts", err))?;
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    Ok(mounts.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let _source = fields.next()?;
        let mount_point = fields.next()?;
        let fs_type = fields.next()?;

        if Path::new(mount_point) == canonical {
            Some(fs_type.to_string())
        } else {
            None
        }
    }))
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::validate_output_path;
    use std::path::Path;

    #[test]
    fn rejects_paths_outside_mount() {
        assert!(
            validate_output_path(Path::new("/run/secrets-bridge"), Path::new("/tmp/key")).is_err()
        );
    }

    #[test]
    fn accepts_relative_paths_under_mount() {
        let path = validate_output_path(Path::new("/run/secrets-bridge"), Path::new("luks/key"))
            .expect("relative path should be accepted");
        assert_eq!(path, Path::new("/run/secrets-bridge/luks/key"));
    }
}
