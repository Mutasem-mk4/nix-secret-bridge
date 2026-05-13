//! Process hardening: disable core dumps, lock memory

use anyhow::{Context, Result};
use tracing::info;

/// Harden the current process against secret leakage
///
/// - Disables core dumps via prctl(PR_SET_DUMPABLE, 0)
/// - Locks current and future memory pages to prevent swapping
pub fn harden_process() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // Disable core dumps
        nix::sys::prctl::set_dumpable(false)
            .context("prctl(PR_SET_DUMPABLE, 0) failed")?;

        // Lock all current and future memory pages
        nix::sys::mman::mlockall(nix::sys::mman::MlockAllFlags::MCL_CURRENT | nix::sys::mman::MlockAllFlags::MCL_FUTURE)
            .context("mlockall failed — secrets may be swapped to disk")?;

        info!("Process hardened: core dumps disabled, memory locked");
    }

    #[cfg(not(target_os = "linux"))]
    {
        tracing::warn!("Process hardening only supported on Linux");
    }

    Ok(())
}
