# Roadmap

Current status: Phase 3 in progress. The Rust CLI, NixOS module, examples, and
VM test definitions are present. Remaining work before upstream submission is
external review, Hydra validation, and the formal RFC process.

## Phase 1: Prototyping

- Define the bootstrap secret threat model and installer-environment scope.
- Implement the Rust CLI with age and SOPS backends.
- Mount `/run/secrets-bridge` as tmpfs and enforce output paths under it.
- Zeroize decrypted buffers and call Linux process hardening primitives.
- Provide `decrypt`, `decrypt-all`, `cleanup`, and `validate-mapping` commands.

Status: complete.

## Phase 2: NixOS Integration

- Package the Rust binary with `rustPlatform.buildRustPackage`.
- Add the NixOS module under `services.nix-secret-bridge`.
- Expose generated provider paths at `config.nix-secret-bridge.providers.*.paths`.
- Order `nix-secret-bridge.service` before `disko.service`.
- Add cleanup after `disko.service`.
- Add a `pkgs/by-name/ni/nix-secret-bridge/package.nix` copy for nixpkgs review.

Status: complete pending maintainer review.

## Phase 3: Testing and Documentation

- Maintain VM integration tests for both age and SOPS backends.
- Exercise the real handoff into `disko` with an encrypted disk.
- Verify tmpfs placement, file permissions, cleanup, and absence of plaintext in
  `/nix/store`.
- Add manual-style documentation and nixos-anywhere examples.
- Run `cargo test`, `nix flake check`, and the VM tests in CI.

Status: in progress.

## Phase 4: RFC and nixpkgs Submission

- Post the finalized RFC to the NixOS RFC repository when review indicates the
  module belongs in the official module set.
- Request review from `disko`, `nixos-anywhere`, NixOS module, and security
  maintainers.
- Submit the package under `pkgs/by-name/ni/nix-secret-bridge`.
- Submit the NixOS module and manual entry in the same or a follow-up PR.
- Address RFC 36 process requirements: clearly scoped motivation, alternatives,
  compatibility impact, unresolved questions, and implementation status.

Status: planned.
