# nix-secret-bridge

`nix-secret-bridge` provides encrypted bootstrap secrets to `disko` during the
installer phase of a NixOS deployment. It exists for the case where an encrypted
disk needs a LUKS key before the target system has booted and before normal
secret managers such as `agenix` or `sops-nix` can run.

## What It Does

- Decrypts age or SOPS files in the installer environment.
- Mounts `/run/secrets-bridge` as tmpfs.
- Writes decrypted bytes atomically with mode `0400`.
- Orders `nix-secret-bridge.service` before `disko.service`.
- Cleans up by overwriting, unlinking, and unmounting after `disko`.
- Keeps plaintext secrets out of `/nix/store` and persistent filesystems.

## Quick Start

```nix
{
  services.nix-secret-bridge = {
    enable = true;
    backend = "age";
    masterKeyPath = "/run/nix-secret-bridge/age-identity.txt";

    secretMapping.luks = {
      encryptedFile = ./secrets/luks-key.age;
      outputPath = "/run/secrets-bridge/luks";
    };
  };

  disko.devices.disk.main.content.partitions.root.content = {
    type = "luks";
    name = "cryptroot";
    settings.keyFile = config.nix-secret-bridge.providers.age.paths.luks;
    content = {
      type = "filesystem";
      format = "ext4";
      mountpoint = "/";
    };
  };
}
```

Deploy with `nixos-anywhere`:

```bash
export NIX_SECRET_BRIDGE_AGE_KEY="$(cat ~/.config/sops/age/keys.txt)"
./examples/deploy.sh .#host root@192.0.2.10
```

## CLI

```bash
nix-secret-bridge decrypt \
  --backend age \
  --encrypted-file ./secrets/luks-key.age \
  --output-path /run/secrets-bridge/luks \
  --master-key-file /run/nix-secret-bridge/age-identity.txt

nix-secret-bridge decrypt-all \
  --mapping-file /nix/store/...-nix-secret-bridge-mapping.json

nix-secret-bridge cleanup --mount-base /run/secrets-bridge
```

## Repository Layout

```text
src/main.rs                  CLI entry point
src/backends/age.rs          age backend using the age crate
src/backends/sops.rs         SOPS backend using the sops binary
src/mount.rs                 tmpfs mount and atomic writes
src/cleanup.rs               zeroize, unlink, and unmount
module.nix                   NixOS module
tests/default.nix            VM integration tests
doc/manual.md                manual-style documentation
examples/                    nixos-anywhere and disko examples
pkgs/by-name/ni/...          nixpkgs package layout copy
```

## Development

```bash
cargo fmt --check
cargo test --locked
nix flake check -L
```

The package supports Linux. The VM tests require Nix with QEMU support and are
intended to run on Linux builders.
