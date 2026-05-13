<p align="center">
  <img src="https://img.shields.io/badge/NixOS-5277C3?style=for-the-badge&logo=nixos&logoColor=white" alt="NixOS" />
  <img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/License-MIT-green?style=for-the-badge" alt="MIT License" />
  <img src="https://img.shields.io/badge/Status-Pre--RFC-orange?style=for-the-badge" alt="Pre-RFC" />
</p>

<h1 align="center">🔐 nix-secret-bridge</h1>

<p align="center">
  <strong>Bootstrap Secret Orchestrator for NixOS Disk Encryption</strong>
  <br />
  <em>Decrypt LUKS keys during disk partitioning — before the system even boots.</em>
</p>

<p align="center">
  <a href="#the-problem">Problem</a> •
  <a href="#how-it-works">How It Works</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#module-options">Module Options</a> •
  <a href="#cli-reference">CLI Reference</a> •
  <a href="#security-model">Security</a> •
  <a href="#contributing">Contributing</a>
</p>

---

## The Problem

Every NixOS deployment with full-disk encryption hits the same wall:

```
$ nixos-anywhere --flake .#myhost root@target

disko: formatting disk /dev/sda
disko: creating LUKS partition 'cryptroot'
cryptsetup: ERROR: Cannot read keyfile /run/secrets/luks-key.
                   No such file or directory.
disko: ✗ FAILED
```

**Why?** Because `disko` needs LUKS keys during `luksFormat`, but secret managers like `sops-nix` and `agenix` only activate as systemd services **after** the system has booted. The target filesystem doesn't even exist yet — we're in the process of creating it.

This is the **bootstrap paradox**: you can't create the encrypted disk without the key, and the key management system can't run until the disk exists.

### Current Workarounds (All Bad)

| Approach | Problem |
|----------|---------|
| 🔴 Hard-code the key in Nix config | Plaintext secret in `/nix/store` — visible to all users |
| 🔴 SSH in and paste it manually | Not reproducible, breaks automation |
| 🔴 Custom pre-disko bash scripts | Fragile, no cleanup, no memory safety |
| 🔴 `nixos-anywhere --extra-files` | Plaintext on deployer's machine, transmitted raw over SSH |

**`nix-secret-bridge` eliminates all of these.**

---

## How It Works

```
                         INSTALLER ENVIRONMENT
                    (Live USB / nixos-anywhere SSH)

  ┌─────────────────┐     ┌──────────────────────┐     ┌──────────────┐
  │  Encrypted Repo  │     │                      │     │   /run/      │
  │                  │────▶│  nix-secret-bridge    │────▶│   secrets-   │
  │  *.age           │     │                      │     │   bridge/    │
  │  *.sops.yaml     │     │  • Decrypt in memory  │     │              │
  └─────────────────┘     │  • Mount tmpfs        │     │  luks-key    │
                          │  • Write (mode 0400)  │     │  (0400 root) │
  ┌─────────────────┐     │  • Zeroize buffer     │     └──────┬───────┘
  │  Master Key      │     └──────────────────────┘            │
  │                  │────▶                                     │
  │  ENV / file /    │           ┌──────────────┐              │
  │  YubiKey         │           │              │◀─────────────┘
  └─────────────────┘           │    disko      │
                                │  luksFormat   │
                                │              │
                                └──────┬───────┘
                                       │
                          ┌────────────▼────────────┐
                          │  nix-secret-bridge       │
                          │  --cleanup               │
                          │                          │
                          │  • Overwrite with zeros  │
                          │  • Unlink files          │
                          │  • Unmount tmpfs         │
                          │  • No trace left.        │
                          └──────────────────────────┘
```

**Three systemd phases, fully automated:**

| Phase | Service | Timing |
|-------|---------|--------|
| **1. Decrypt** | `nix-secret-bridge-installer.service` | `Before=disko.service` |
| **2. Consume** | `disko.service` | Reads `/run/secrets-bridge/*` |
| **3. Cleanup** | `nix-secret-bridge-cleanup.service` | `After=disko.service` |

---

## Quick Start

### 1. Encrypt your LUKS key

```bash
# Generate a random 4096-byte LUKS key
dd if=/dev/urandom of=luks-key bs=4096 count=1

# Encrypt it with your age public key
age -r age1yourkeyhere... -o secrets/luks-key.age luks-key

# Destroy the plaintext — it's never needed again
shred -u luks-key
```

### 2. Add to your flake

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    disko.url = "github:nix-community/disko";
    nix-secret-bridge.url = "github:Mutasem-mk4/nix-secret-bridge";
  };

  outputs = { self, nixpkgs, disko, nix-secret-bridge, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        disko.nixosModules.disko
        nix-secret-bridge.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

### 3. Configure the bridge + disko

```nix
# configuration.nix
{ config, ... }:
{
  # ── Secret Bridge: decrypt LUKS key before disko runs ──────────
  services.nix-secret-bridge = {
    enable = true;
    backend = "age";                    # or "sops"

    secretMapping.luks-key = {
      encryptedFile = ./secrets/luks-key.age;
      outputPath = "/run/secrets-bridge/luks-key";
    };

    diskoIntegration = true;            # auto Before=disko.service
  };

  # ── Disko: consume the decrypted key ──────────────────────────
  disko.devices.disk.main = {
    type = "disk";
    device = "/dev/sda";
    content = {
      type = "gpt";
      partitions = {
        boot = {
          size = "512M";
          type = "EF00";
          content = {
            type = "filesystem";
            format = "vfat";
            mountpoint = "/boot";
          };
        };
        root = {
          size = "100%";
          content = {
            type = "luks";
            name = "cryptroot";
            settings = {
              keyFile = "/run/secrets-bridge/luks-key";  # ← provided by nix-secret-bridge
              keyFileSize = 4096;
            };
            content = {
              type = "filesystem";
              format = "ext4";
              mountpoint = "/";
            };
          };
        };
      };
    };
  };
}
```

### 4. Deploy

```bash
# Provide your age identity for decryption
export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)

# Deploy — nix-secret-bridge handles everything automatically
nixos-anywhere --flake .#myhost root@192.168.1.100
```

**That's it.** The LUKS partition is created, the key is used and destroyed. No manual steps.

---

## Features

| Feature | Description |
|---------|-------------|
| 🔌 **Backend Agnostic** | Supports **age** (agenix-compatible) and **SOPS** (sops-nix-compatible) out of the box. Pluggable `SecretBackend` trait for custom backends. |
| 🦀 **Memory Safe** | Written in Rust. All decrypted buffers wrapped in `Zeroizing<Vec<u8>>` — automatically overwritten with zeros on drop. |
| 💾 **Tmpfs Only** | Decrypted secrets live exclusively on tmpfs (`noswap,noexec,nosuid,nodev`). Nothing touches persistent storage. Ever. |
| 🛡️ **Process Hardened** | Core dumps disabled (`PR_SET_DUMPABLE=0`), memory locked (`mlock()`), `panic="abort"` to prevent stack unwinding leaks. |
| 🧹 **Automatic Cleanup** | After disko finishes: file contents overwritten with zeros → files unlinked → tmpfs unmounted → mount point removed. |
| 🔑 **No Re-keying** | Uses your **existing** age/sops keys from your repository. No new key generation, no manual `rekey` step. |
| ⚙️ **NixOS Native** | Full NixOS module with `mkOption`, `mkIf`, systemd service ordering, and hardened unit configuration. |
| 🤝 **Complementary** | Works alongside `disko`, `nixos-anywhere`, `agenix`, and `sops-nix` — replaces none of them. |

---

## Module Options

```nix
services.nix-secret-bridge = {
  # Enable the bootstrap secret orchestrator
  enable = true;                           # default: false

  # Decryption backend ("age" or "sops")
  backend = "age";                         # default: "age"

  # Map of secrets: name → { encryptedFile, outputPath, backend? }
  secretMapping = {
    luks-key = {
      encryptedFile = ./secrets/luks-key.age;
      outputPath = "/run/secrets-bridge/luks-key";
      # backend = "sops";                  # override per-secret
    };
  };

  # Master key for non-interactive decryption
  provider.masterKeyPath = null;           # or "/tmp/age-key.txt"

  # Auto-hook into disko via systemd Before=disko.service
  diskoIntegration = true;                 # default: true

  # Auto-cleanup after disko (zeroize + unmount)
  cleanupAfterDisko = true;                # default: true

  # Base tmpfs mount point
  mountBase = "/run/secrets-bridge";       # default
};
```

---

## CLI Reference

### `nix-secret-bridge decrypt`

Decrypt a single secret and place it on tmpfs.

```bash
nix-secret-bridge decrypt \
  --backend age \
  --encrypted-file ./secrets/luks-key.age \
  --output-path /run/secrets-bridge/luks-key \
  --master-key-file /tmp/age-identity.txt
```

### `nix-secret-bridge decrypt-all`

Decrypt multiple secrets from a JSON mapping file.

```bash
nix-secret-bridge decrypt-all \
  --mapping-file /etc/nix-secret-bridge/mapping.json \
  --mount-base /run/secrets-bridge
```

### `nix-secret-bridge cleanup`

Securely remove all decrypted secrets.

```bash
nix-secret-bridge cleanup --mount-base /run/secrets-bridge
```

### Key Provisioning

| Method | Usage |
|--------|-------|
| **Environment variable** | `export NIX_SECRET_BRIDGE_AGE_KEY=$(cat key.txt)` |
| **File path** | `--master-key-file /tmp/age-identity.txt` |
| **SOPS env** | `export SOPS_AGE_KEY_FILE=/path/to/keys.txt` |
| **YubiKey** | `--yubikey` (via `age-plugin-yubikey`) |
| **Interactive** | `--interactive` (passphrase prompt) |

---

## Security Model

### Threat Mitigations

| Threat | Mitigation |
|--------|-----------|
| Secret persisted to disk | Decrypted data exists **only** on tmpfs. Mounted with `noswap`. |
| Secret in swap | Process memory locked via `mlock()`. tmpfs `noswap` flag. |
| Secret in logs | All logging sanitized. systemd `StandardOutput=null`. |
| Secret in core dump | `prctl(PR_SET_DUMPABLE, 0)` at process start. |
| Secret in `/nix/store` | Encrypted files may be in store (that's safe). Decrypted content is **never** part of a derivation. |
| Use-after-free | Rust ownership + `Zeroizing<Vec<u8>>` auto-wipe on drop. |
| Race condition on tmpfs | Files created with `O_EXCL \| O_CREAT`, mode set before write via `fchmod`. |
| Stack unwinding leak | Compiled with `panic = "abort"`. |

### What IS Safe

- ✅ Encrypted `.age` / `.sops` files in `/nix/store` — they're encrypted
- ✅ The `nix-secret-bridge` binary in `/nix/store` — it's just code
- ✅ The NixOS module configuration — it references paths, not secrets

### What Is NEVER Written to Disk

- ❌ Decrypted key material (tmpfs + memory only)
- ❌ Master keys (env vars or ephemeral files via SSH)
- ❌ Intermediate decryption artifacts

---

## Advanced Usage

### Using SOPS Backend

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "sops";

  secretMapping.luks-key = {
    encryptedFile = ./secrets/luks-key.sops;
    outputPath = "/run/secrets-bridge/luks-key";
  };

  provider.masterKeyPath = "/tmp/age-identity.txt";
};
```

### YubiKey Support

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "age";
  secretMapping.luks-key = {
    encryptedFile = ./secrets/luks-key.age;
    outputPath = "/run/secrets-bridge/luks-key";
  };
  # YubiKey identity is auto-detected via age-plugin-yubikey
};
```

### Multiple Encrypted Disks

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "age";

  secretMapping = {
    luks-root = {
      encryptedFile = ./secrets/root-key.age;
      outputPath = "/run/secrets-bridge/root-key";
    };
    luks-data = {
      encryptedFile = ./secrets/data-key.age;
      outputPath = "/run/secrets-bridge/data-key";
      backend = "sops";  # override backend per-secret
    };
  };
};
```

### Mixed Backends (age + SOPS)

```nix
secretMapping = {
  root-key = {
    encryptedFile = ./secrets/root.age;
    outputPath = "/run/secrets-bridge/root";
    backend = "age";
  };
  data-key = {
    encryptedFile = ./secrets/data.sops.yaml;
    outputPath = "/run/secrets-bridge/data";
    backend = "sops";
  };
};
```

---

## Integration Points

```
┌──────────────────────────────────────────────────────────────┐
│                    nix-secret-bridge                          │
│              (pre-boot secret decryption)                     │
└──────────┬────────────────┬──────────────────┬───────────────┘
           │                │                  │
     ┌─────▼─────┐   ┌─────▼──────┐   ┌──────▼──────┐
     │   disko    │   │  nixos-    │   │  agenix /   │
     │            │   │  anywhere  │   │  sops-nix   │
     │ Consumes   │   │ Provides   │   │ Same keys,  │
     │ keys from  │   │ SSH env    │   │ post-boot   │
     │ tmpfs      │   │ for keys   │   │ secrets     │
     └────────────┘   └────────────┘   └─────────────┘
```

| Tool | Relationship |
|------|-------------|
| **disko** | Complementary — consumes decrypted keys from `/run/secrets-bridge/` |
| **nixos-anywhere** | Complementary — establishes SSH session, forwards env vars |
| **agenix** | Complementary — same encrypted files, handles post-boot secrets |
| **sops-nix** | Complementary — same keys, same relationship as agenix |
| **nixos-facter** | Future — TPM2 PCR values for hardware-bound key unsealing |

---

## Development

```bash
# Enter the development shell (includes Rust, Nix tools, age, sops)
nix develop

# Build
cargo build

# Run unit tests
cargo test

# Run NixOS VM integration tests (requires Linux + KVM)
nix flake check

# Format
cargo fmt
nixpkgs-fmt .

# Lint
cargo clippy -- -D warnings
statix check .
deadnix .
```

### Project Structure

```
nix-secret-bridge/
├── src/
│   ├── main.rs                  # CLI entry point (clap)
│   ├── error.rs                 # Error types
│   ├── mount.rs                 # tmpfs mounting + secure file write
│   ├── cleanup.rs               # Zeroize → unlink → umount
│   ├── security.rs              # mlock, prctl hardening
│   └── backends/
│       ├── mod.rs               # SecretBackend trait + factory
│       ├── age_backend.rs       # Pure-Rust age via rage crate
│       └── sops_backend.rs      # SOPS via CLI
├── module.nix                   # NixOS module (mkOption, systemd)
├── package.nix                  # Nix package (pkgs/by-name ready)
├── flake.nix                    # Flake outputs
├── tests/default.nix            # NixOS VM integration tests
├── doc/manual.md                # NixOS manual documentation
├── DESIGN.md                    # Architecture & security model
├── RFC.md                       # Pre-RFC for NixOS Discourse
└── ROADMAP.md                   # 4-phase contribution plan
```

---

## Roadmap

| Phase | Timeline | Status | Focus |
|-------|----------|--------|-------|
| **Phase 1** | Week 1–3 | 🟡 In Progress | Rust crate, backends, CLI, unit tests |
| **Phase 2** | Week 4–6 | ⚪ Planned | NixOS module, systemd integration |
| **Phase 3** | Week 7–9 | ⚪ Planned | VM tests, documentation |
| **Phase 4** | Week 10–12 | ⚪ Planned | Pre-RFC → feedback → PR → merge |

See [ROADMAP.md](./ROADMAP.md) for detailed task breakdown.

---

## Troubleshooting

<details>
<summary><strong>❌ "Master key not found"</strong></summary>

Ensure one of these is set:

```bash
# Option 1: Environment variable
export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)

# Option 2: File path
nix-secret-bridge decrypt --master-key-file /path/to/identity.txt ...

# Option 3: In NixOS config
services.nix-secret-bridge.provider.masterKeyPath = "/tmp/age-key.txt";
```

</details>

<details>
<summary><strong>❌ "sops binary not found in PATH"</strong></summary>

The SOPS backend requires `sops` in the environment:

```nix
environment.systemPackages = [ pkgs.sops ];
```

The module does this automatically when `backend = "sops"`.

</details>

<details>
<summary><strong>❌ "Failed to mount tmpfs"</strong></summary>

`nix-secret-bridge` requires root privileges to mount tmpfs. When using `nixos-anywhere`, this is automatic. For manual use:

```bash
sudo nix-secret-bridge decrypt ...
```

</details>

<details>
<summary><strong>❌ "Decryption failed: wrong key"</strong></summary>

Verify you're using the correct age identity:

```bash
# Check which public key the file was encrypted for
age --decrypt --identity /dev/null secrets/luks-key.age 2>&1 | head -5

# Compare with your identity's public key
grep "public key:" ~/.config/sops/age/keys.txt
```

</details>

---

## Contributing

This project is in the **Pre-RFC** phase. We welcome:

- 🐛 Bug reports and feature requests via [Issues](https://github.com/Mutasem-mk4/nix-secret-bridge/issues)
- 💬 Design feedback on the [Pre-RFC draft](./RFC.md)
- 🔧 Pull requests (especially for backend plugins and test coverage)

See [RFC.md](./RFC.md) for the full design rationale and [DESIGN.md](./DESIGN.md) for architecture details.

---

## License

[MIT](./LICENSE) — Mutasem Kharma © 2026
