# Security and Threat Model

`nix-secret-bridge` is designed to be a highly secure bootstrap orchestrator. It bridges the gap between encrypted secret files (e.g., SOPS, age) and formatting tools (e.g., `disko`, `cryptsetup`) that require plaintext credentials during NixOS disk partitioning.

## Security Guarantees

1. **Zero Disk Leakage**: Decrypted secrets are placed exclusively on a strictly locked-down `tmpfs` mount point (`noexec`, `nosuid`, `nodev`, `noswap`). Secrets never touch the persistent disk or `/nix/store/`.
2. **Memory Safety**: 
   - All buffers containing plaintext secrets are wrapped in the `zeroize` crate to guarantee they are overwritten with zeroes upon dropping.
   - The process explicitly uses `mlockall(MCL_CURRENT | MCL_FUTURE)` to lock process memory, preventing the kernel from paging secrets out to swap.
3. **Core Dump Prevention**: Core dumps are disabled via `prctl(PR_SET_DUMPABLE, 0)` so crashes do not leak secrets to `/var/lib/systemd/coredump`.
4. **NixOS Module Hardening**:
   - Master decryption keys are passed natively via systemd's `LoadCredential=` securely over a file descriptor, ensuring they cannot be snooped via environment variables or `ps aux`.
   - The module asserts at evaluation time that your master key file is **not** accidentally copied into the world-readable `/nix/store/`.
   - Execution occurs under `ProtectSystem=strict`, `MemoryDenyWriteExecute=true`, and with locked down standard output (`StandardOutput=null`) to prevent logging leakage.

## Out of Scope (Assumptions)

- **Rootkit/Installer Compromise**: `nix-secret-bridge` runs as root in the installer environment to mount tmpfs and supply secrets to block devices. It assumes the installer ISO/environment is fundamentally trustworthy. If your installer environment is compromised, a rootkit can attach a debugger (e.g., `ptrace`) or use eBPF to dump memory.
- **Hardware-level attacks**: Cold boot attacks or malicious hardware (e.g., hardware keyloggers for interactive passphrase entry, malicious DMA attacks) are outside the scope of software-based protections.

## Reporting Vulnerabilities

If you discover a vulnerability or security flaw, please do not open a public issue. Reach out directly to the maintainers to coordinate a patch.
