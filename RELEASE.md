# Release Checklist

Use this checklist for every tagged release.

1. Confirm the working tree is clean.
2. Run `cargo fmt --check`.
3. Run `cargo clippy --locked -- -D warnings`.
4. Run `cargo test --locked`.
5. Run `cargo deny check advisories bans sources`.
6. Run `nix flake check -L` on Linux.
7. Update `CHANGELOG.md` and move relevant entries out of `Unreleased`.
8. Bump `Cargo.toml` and any Nix package versions in the same commit.
9. Create an annotated tag, for example `git tag -a v0.1.1 -m "v0.1.1"`.
10. Push the branch and tag.
11. Publish GitHub release notes from the changelog.
12. For nixpkgs updates, refresh `hash` and `cargoHash` from a Linux Nix build.

Do not tag a release when known failing security advisories are present unless
the release notes explain the advisory, impact, and mitigation.
