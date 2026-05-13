# Changelog

All notable changes to `nix-secret-bridge` are documented here.

The project follows semantic versioning while the CLI and module interfaces are
stabilizing. Breaking changes before `1.0.0` are possible, but releases should
call them out explicitly.

## Unreleased

- Fail decrypt operations when Linux process hardening cannot be applied.
- Clarify the threat model and current limitations for security-sensitive use.
- Add Clippy and dependency advisory checks to CI.
- Add a Cargo dependency policy for RustSec advisories, yanked crates, unknown
  registries, and wildcard dependencies.
- Remove unused age SSH/plugin features to avoid carrying unsupported key-source
  paths and the vulnerable `rsa` transitive dependency.

## 0.1.0 - 2026-05-13

- Initial Rust CLI with age and SOPS backends.
- Add tmpfs handoff for plaintext bootstrap secrets.
- Add cleanup command for zeroing, unlinking, and unmounting tmpfs secrets.
- Add NixOS module integration for installer and disko workflows.
- Add VM integration test definitions for age and SOPS backends.
