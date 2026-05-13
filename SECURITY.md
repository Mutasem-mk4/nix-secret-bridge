# Security and Threat Model

`nix-secret-bridge` is a bootstrap secret handoff tool for the NixOS installer environment. It bridges encrypted secret files such as age or SOPS data to formatting tools such as `disko` and `cryptsetup`, which need plaintext credentials before the installed system can boot.

## Security Guarantees

1. **Zero Disk Leakage**: Decrypted secrets are placed exclusively on a strictly locked-down `tmpfs` mount point (`noexec`, `nosuid`, `nodev`, `noswap`). Secrets never touch the persistent disk or `/nix/store/`.
2. **Memory Safety**:
   - All buffers containing plaintext secrets are wrapped in the `zeroize` crate to guarantee they are overwritten with zeroes upon dropping.
   - On Linux, decrypt operations fail unless `prctl(PR_SET_DUMPABLE, 0)` and `mlockall(MCL_CURRENT | MCL_FUTURE)` succeed.
3. **Core Dump Prevention**: Core dumps are disabled before decrypting secret material so crashes do not leak secrets to `/var/lib/systemd/coredump`.
4. **NixOS Module Hardening**:
   - Master decryption keys are referenced by runtime paths under `/run` or by a runtime environment file. They must not be imported as Nix paths.
   - The module asserts at evaluation time that your master key file is **not** accidentally copied into the world-readable `/nix/store/`.
   - Execution occurs under `ProtectSystem=strict`, `MemoryDenyWriteExecute=true`, and with locked down standard output (`StandardOutput=null`) to prevent logging leakage.

## Current Limitations

- The project is young and has not yet received an external security audit.
- The SOPS backend shells out to the `sops` binary and relies on the installed `sops` implementation and its supported key sources.
- Hardware-backed age identities may work when the required age plugin is present in the installer environment, but this is not yet covered by automated VM tests.
- The threat model assumes the installer environment and root account are trusted during deployment.

## Out of Scope (Assumptions)

- **Rootkit/Installer Compromise**: `nix-secret-bridge` runs as root in the installer environment to mount tmpfs and supply secrets to block devices. It assumes the installer ISO/environment is fundamentally trustworthy. If your installer environment is compromised, privileged malware can observe plaintext secrets.
- **Hardware-level attacks**: Cold boot attacks or malicious hardware (e.g., hardware keyloggers for interactive passphrase entry, malicious DMA attacks) are outside the scope of software-based protections.

## Reporting Vulnerabilities

If you discover a vulnerability or security flaw, please do not open a public issue. Reach out directly to the maintainers to coordinate a patch.
