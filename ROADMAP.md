# Contribution Roadmap — nix-secret-bridge

## Phase 1: Rust Core (Weeks 1–3)

### Goals
- [ ] Complete Rust crate with age and SOPS decryption backends
- [ ] tmpfs mounting with hardened mount options
- [ ] Secure cleanup (zeroize + unlink + umount)
- [ ] CLI with `decrypt`, `decrypt-all`, and `cleanup` subcommands
- [ ] Process hardening (mlock, prctl, panic=abort)
- [ ] Unit tests for all backends and error paths
- [ ] `cargo clippy` and `cargo fmt` clean

### Deliverables
- `Cargo.toml`, `src/` with all modules
- `cargo test` passes
- No `unwrap()` in production code paths

---

## Phase 2: NixOS Module + systemd Integration (Weeks 4–6)

### Goals
- [ ] NixOS module with full `mkOption` specification
- [ ] systemd service: `nix-secret-bridge-installer.service`
- [ ] systemd service: `nix-secret-bridge-cleanup.service`
- [ ] disko integration via `Before=disko.service` ordering
- [ ] `package.nix` for `pkgs/by-name` structure
- [ ] `flake.nix` with `packages`, `nixosModules`, `checks` outputs
- [ ] Nix expression formatting with `nixpkgs-fmt`

### Deliverables
- `module.nix` with all options documented
- `package.nix` ready for nixpkgs submission
- `flake.nix` with proper input follows

---

## Phase 3: Testing + Documentation (Weeks 7–9)

### Goals
- [ ] NixOS VM test: age backend + disko LUKS
- [ ] NixOS VM test: SOPS backend + disko LUKS
- [ ] Tests verify: LUKS format success, store purity, cleanup
- [ ] Tests run on Hydra (sandboxed, no network after inputs)
- [ ] Manual documentation (Markdown, NixOS manual style)
- [ ] In-code documentation (rustdoc for all public items)
- [ ] README with quick start and architecture overview

### Deliverables
- `tests/default.nix` with ≥2 passing VM tests
- `doc/manual.md` for NixOS manual integration
- `README.md` with badges and examples

---

## Phase 4: Community Process (Weeks 10–12)

### Goals
- [ ] Post Pre-RFC to NixOS Discourse
- [ ] Incorporate community feedback (≥2 weeks comment period)
- [ ] Submit formal RFC if required by scope
- [ ] Submit PR to `nix-community` organization
- [ ] Request review from disko/nixos-anywhere maintainers
- [ ] Address review feedback
- [ ] Merge into `nix-community` (stepping stone for nixpkgs)

### Governance Checkpoints
- [ ] Verify compliance with RFC 36 process
- [ ] Reference 2024/2025 Community Survey data
- [ ] Obtain ≥2 maintainer reviews
- [ ] Pass Hydra CI for all supported platforms
- [ ] `pkgs/by-name` structure validated

### Success Criteria
- PR merged into `nix-community/nix-secret-bridge`
- Listed in awesome-nix under "Secret Management"
- At least one external user reports successful deployment
- Integration test green on Hydra for x86_64-linux and aarch64-linux
