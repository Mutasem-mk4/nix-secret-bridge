# Pre-RFC: nix-secret-bridge — Bootstrap Secret Orchestration for NixOS Disk Encryption

**Author**: Mutasem Kharma  
**Date**: 2026-05-13  
**Status**: Pre-RFC (seeking community feedback)  

---

## Summary

Introduce `nix-secret-bridge`, a Rust CLI tool and NixOS module that decrypts
secrets during the disk partitioning phase (before system boot) so that `disko`
can consume LUKS passwords and encryption keys — solving the "bootstrap secret
paradox."

---

## Motivation

### The Bootstrap Paradox

Every NixOS user deploying full-disk encryption with `disko` hits the same wall:

```
$ nixos-anywhere --flake .#myhost root@target
...
disko: formatting disk /dev/sda
disko: creating LUKS partition 'cryptroot'
cryptsetup: ERROR: Cannot read keyfile /run/secrets/luks-key.
                   No such file or directory.
disko: FAILED
```

The failure is fundamental. `disko` runs `cryptsetup luksFormat --key-file
/run/secrets/luks-key`, but `/run/secrets/luks-key` doesn't exist because:

1. `sops-nix` / `agenix` are systemd services that activate **after boot**
2. `disko` runs **before boot** — in the installer environment
3. The target root filesystem doesn't exist yet (we're creating it)

Users are forced into one of these workarounds:

- **Manual**: SSH in, paste the key, partition manually, then run `nixos-install`
- **Insecure**: Hard-code the key in the Nix configuration (plaintext in `/nix/store`)
- **Fragile**: Write custom bash scripts that run before disko

None of these are reproducible, secure, or declarative — the core values of NixOS.

### Community Demand

The 2024 Nix Community Survey and 2025 Ecosystem Report both highlight "secrets
management" as the #1 pain point, with "disk encryption bootstrap" specifically
called out in multiple Discourse threads:

- [Discourse: LUKS + disko + sops-nix integration](https://discourse.nixos.org/t/...)
- [GitHub: disko #347 — keyFile from encrypted source](https://github.com/nix-community/disko/issues/347)
- [GitHub: nixos-anywhere #89 — secrets during partitioning](https://github.com/nix-community/nixos-anywhere/issues/89)

### Strategic Alignment

Per RFC 36 governance and the 2025 Technical Direction, `nix-secret-bridge`:

- Follows the **modular design** principle (standalone tool, NixOS module, clean interfaces)
- Maintains **Nix store purity** (no decrypted secrets in `/nix/store`)
- Is **backend-agnostic** (age, SOPS, pluggable)
- Requires **no re-keying** (uses existing keys from the user's repository)
- Integrates with **existing tools** (`disko`, `nixos-anywhere`, `agenix`, `sops-nix`)

---

## Detailed Design

### Architecture

`nix-secret-bridge` operates in three phases:

```
┌─ Installer Environment ────────────────────────────────────┐
│                                                             │
│  Phase 1: DECRYPT                                           │
│  nix-secret-bridge-installer.service                       │
│  Before=disko.service                                       │
│                                                             │
│  ┌─────────────┐    ┌────────────────┐    ┌──────────────┐ │
│  │ *.age files  │ →  │ nix-secret-    │ →  │ /run/secrets │ │
│  │ (encrypted)  │    │ bridge decrypt │    │ -bridge/     │ │
│  │              │    │                │    │ (tmpfs 0400) │ │
│  └─────────────┘    └────────────────┘    └──────┬───────┘ │
│                                                   │         │
│  Phase 2: CONSUME                                 │         │
│  disko.service                                    │         │
│                                                   ▼         │
│  cryptsetup luksFormat --key-file /run/secrets-bridge/key   │
│                                                             │
│  Phase 3: CLEANUP                                           │
│  nix-secret-bridge-cleanup.service                         │
│  After=disko.service                                        │
│                                                             │
│  zeroize → unlink → umount                                  │
└─────────────────────────────────────────────────────────────┘
```

### Security Model

| Property | Implementation |
|----------|---------------|
| No persistent secrets | tmpfs with `noswap` flag |
| No secrets in Nix store | Decryption at runtime only |
| Memory safety | Rust + `zeroize` crate + `mlock()` |
| No core dumps | `prctl(PR_SET_DUMPABLE, 0)` |
| No log leakage | Sanitized logging, `StandardOutput=null` |
| Race-free file creation | `O_EXCL | O_CREAT`, `fchmod` before write |

### NixOS Module Interface

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "age";  # or "sops"

  secretMapping = {
    luks-key = {
      encryptedFile = ./secrets/luks-key.age;
      outputPath = "/run/secrets-bridge/luks-key";
    };
  };

  provider.masterKeyPath = null;  # use env var
  diskoIntegration = true;        # auto Before=disko.service
};
```

### Full design document

See [DESIGN.md](./DESIGN.md) in the repository for the complete architecture,
security threat model, backend plugin system, and integration details.

---

## Interaction with Existing Tools

| Tool | Relationship |
|------|-------------|
| **disko** | Complementary. `nix-secret-bridge` provides secrets that disko consumes. Integration via systemd ordering. |
| **nixos-anywhere** | Complementary. nixos-anywhere establishes the SSH session; nix-secret-bridge handles secret decryption within it. |
| **agenix** | Complementary. Same encrypted files and keys. agenix handles post-boot secrets; nix-secret-bridge handles pre-boot. |
| **sops-nix** | Complementary. Same relationship as agenix. SOPS backend shells out to the `sops` CLI. |
| **nixos-facter** | Future integration. Could use hardware facts (TPM2 PCR values) for key unsealing. |

---

## Alternatives Considered

### 1. Patching disko to decrypt secrets directly

**Rejected.** Mixing concerns: disko is a disk partitioning tool, not a secrets
manager. Adding decryption to disko would increase its attack surface and
maintenance burden. The modular approach (separate tool, clean interface) is
more aligned with NixOS design principles.

### 2. Using `age --decrypt` in a pre-disko script

**Partially addressed.** This is essentially what `nix-secret-bridge` does, but
in a principled way: with proper tmpfs mounting, zeroization, cleanup, NixOS
module integration, and multiple backend support. The raw `age` approach lacks
security hardening and integration with the NixOS module system.

### 3. Copying secrets via SSH (nixos-anywhere --extra-files)

**Insufficient.** This requires the plaintext secret to exist on the deployer's
machine and be transmitted over SSH. It doesn't support encrypted-at-rest
secrets in the repository, and doesn't provide cleanup or memory safety.

### 4. Using systemd-creds

**Future work.** `systemd-creds` (systemd 250+) provides encrypted credentials
tied to the machine's TPM2. This is compelling but doesn't solve the initial
bootstrap: the machine doesn't have a TPM-sealed credential store until after
the first boot. `nix-secret-bridge` could be extended to seal secrets into
systemd-creds after the first boot.

---

## Unresolved Questions

1. **TPM2 integration via nixos-facter**: Should `nix-secret-bridge` support
   TPM2-sealed keys for re-deployment scenarios where the hardware is known?

2. **Multiple disk support**: The current design handles multiple secrets via
   `secretMapping`, but should there be first-class support for multi-disk RAID
   or ZFS configurations?

3. **Plugin architecture**: The design includes a plugin system for custom
   backends. Should this be in v0.1 or deferred to v0.2?

4. **Upstream integration path**: Should this land in `nix-community` first
   (as a flake) or go directly to `nixpkgs` (via `pkgs/by-name`)?

---

## Usage Example

```nix
# Full working example: encrypted root with disko + nix-secret-bridge
{
  services.nix-secret-bridge = {
    enable = true;
    backend = "age";
    secretMapping.luks-root = {
      encryptedFile = ./secrets/luks-root.age;
      outputPath = "/run/secrets-bridge/luks-root";
    };
  };

  disko.devices.disk.main = {
    type = "disk";
    device = "/dev/sda";
    content.type = "gpt";
    content.partitions = {
      esp = {
        size = "512M";
        type = "EF00";
        content = { type = "filesystem"; format = "vfat"; mountpoint = "/boot"; };
      };
      root = {
        size = "100%";
        content = {
          type = "luks";
          name = "cryptroot";
          settings.keyFile = "/run/secrets-bridge/luks-root";
          settings.keyFileSize = 4096;
          content = { type = "filesystem"; format = "ext4"; mountpoint = "/"; };
        };
      };
    };
  };
}
```

Deploy:
```bash
export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)
nixos-anywhere --flake .#myhost root@192.168.1.100
```

---

## Contribution Roadmap

| Phase | Timeline | Deliverable |
|-------|----------|------------|
| 1 | Week 1-3 | Rust crate: age/sops decryption, tmpfs mounting, CLI, unit tests |
| 2 | Week 4-6 | NixOS module, systemd integration, disko ordering |
| 3 | Week 7-9 | VM tests (2 backends), manual documentation |
| 4 | Week 10-12 | Discourse Pre-RFC → feedback → formal RFC → PR submission |

---

*Feedback welcome. Please reply with questions, concerns, or endorsements.
I'm particularly interested in feedback on the security model and the
interaction with existing tools.*
