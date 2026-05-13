pub fn harden_process() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        nix::sys::prctl::set_dumpable(false)
            .map_err(|err| format!("prctl(PR_SET_DUMPABLE, 0) failed: {err}"))?;

        let flags =
            nix::sys::mman::MlockAllFlags::MCL_CURRENT | nix::sys::mman::MlockAllFlags::MCL_FUTURE;
        nix::sys::mman::mlockall(flags).map_err(|err| format!("mlockall failed: {err}"))?;

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("process hardening is only implemented on Linux".to_string())
    }
}
