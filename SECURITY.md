# Security and Threat Model

`nix-secret-bridge` is a bootstrap secret handoff tool for the NixOS installer environment. It bridges encrypted secret files such as age or SOPS data to formatting tools such as `disko` and `cryptsetup`, which need plaintext credentials before the installed system can boot.

## Supported Versions

The project is pre-`1.0.0`. Security fixes are released for the latest tagged
minor version only. Users should track the latest release when using
`nix-secret-bridge` for real deployments.

| Version | Security support |
| --- | --- |
| Latest release | Supported |
| Older releases | Best effort only |

## Reporting Vulnerabilities

Please do not open a public issue for suspected vulnerabilities.

Report security issues by emailing the maintainer at `mutasem@kharma.dev` with:

- affected version or commit
- deployment context
- impact description
- reproduction steps or proof of concept, if available

Expected handling:

- Initial response target: 7 days.
- Triage target: 14 days after enough detail is available.
- Fix target: depends on severity and complexity; critical issues are prioritized
  over feature work.

Public disclosure should happen after a fix or mitigation is available, unless
coordinated disclosure requires a different timeline.

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
- Hardware-backed age plugin identities and SSH age identities are not currently supported by the native age backend. Add support only after adding automated tests and reviewing the extra dependency surface.
- The threat model assumes the installer environment and root account are trusted during deployment.

## Out of Scope (Assumptions)

- **Rootkit/Installer Compromise**: `nix-secret-bridge` runs as root in the installer environment to mount tmpfs and supply secrets to block devices. It assumes the installer ISO/environment is fundamentally trustworthy. If your installer environment is compromised, privileged malware can observe plaintext secrets.
- **Hardware-level attacks**: Cold boot attacks or malicious hardware (e.g., hardware keyloggers for interactive passphrase entry, malicious DMA attacks) are outside the scope of software-based protections.
