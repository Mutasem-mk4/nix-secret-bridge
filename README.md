# nix-secret-bridge

**Bootstrap secret orchestrator for NixOS disk encryption.**

Solves the "bootstrap paradox": decrypting LUKS keys during disk partitioning,
before the target system boots, so `disko` can consume them.

## The Problem

```
disko → cryptsetup luksFormat --key-file /run/secrets/luks-key
                                         ^^^^^^^^^^^^^^^^^^^^^^^^
                                         This doesn't exist yet!
```

`sops-nix` and `agenix` run AFTER boot. `disko` runs BEFORE boot. Nothing
bridges the gap — until now.

## Quick Start

```nix
# flake.nix
{
  inputs.nix-secret-bridge.url = "github:nix-community/nix-secret-bridge";
  
  outputs = { nixpkgs, nix-secret-bridge, disko, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        disko.nixosModules.disko
        nix-secret-bridge.nixosModules.default
        {
          services.nix-secret-bridge = {
            enable = true;
            backend = "age";
            secretMapping.luks-key = {
              encryptedFile = ./secrets/luks-key.age;
              outputPath = "/run/secrets-bridge/luks-key";
            };
          };

          disko.devices.disk.main.content.partitions.root.content = {
            type = "luks";
            name = "cryptroot";
            settings.keyFile = "/run/secrets-bridge/luks-key";
          };
        }
      ];
    };
  };
}
```

```bash
export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)
nixos-anywhere --flake .#myhost root@target
```

## Features

- **Backend agnostic**: Supports `age` (agenix-compatible) and `SOPS` (sops-nix-compatible)
- **Memory safe**: Rust + `zeroize` crate, all secrets zeroized on drop
- **Tmpfs only**: Decrypted secrets never touch persistent storage
- **Hardened**: No core dumps, memory locked, no secret logging
- **Automatic cleanup**: Secrets are zeroized and tmpfs unmounted after disko
- **No re-keying**: Uses your existing age/sops keys — no extra setup

## Security Model

| Guarantee | Implementation |
|-----------|---------------|
| No disk persistence | tmpfs with noswap, noexec, nosuid, nodev |
| No Nix store leaks | Runtime-only decryption, never in derivations |
| Memory protection | `mlock()`, `zeroize`, `PR_SET_DUMPABLE=0` |
| Minimal attack surface | Small Rust binary, auditable dependency tree |

## CLI Usage

```bash
# Decrypt a single secret
nix-secret-bridge decrypt \
  --backend age \
  --encrypted-file ./secrets/luks-key.age \
  --output-path /run/secrets-bridge/luks-key \
  --master-key-file /tmp/age-identity.txt

# Decrypt all secrets from a mapping file
nix-secret-bridge decrypt-all \
  --mapping-file /etc/nix-secret-bridge/mapping.json

# Clean up after disko
nix-secret-bridge cleanup --mount-base /run/secrets-bridge
```

## Development

```bash
nix develop  # Enter dev shell with Rust + Nix tools

cargo build  # Build the CLI
cargo test   # Run unit tests
nix flake check  # Run NixOS VM integration tests
```

## Architecture

See [DESIGN.md](./DESIGN.md) for the full design document including:
- Security threat model
- Backend plugin architecture
- Integration with disko/nixos-anywhere
- Detailed data flow diagrams

## Contributing

See [RFC.md](./RFC.md) for the Pre-RFC draft and contribution roadmap.

## License

MIT
