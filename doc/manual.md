# Providing Secrets During Disk Encryption (nix-secret-bridge)

## Problem Statement

When deploying NixOS with full-disk encryption via `disko`, the system needs
access to LUKS passphrases or key files during the `luksFormat` phase. This
phase occurs in the **installer environment** — either a live USB session or an
SSH connection established by `nixos-anywhere` — **before** the target system
has booted.

Existing NixOS secret management tools (`sops-nix`, `agenix`) are designed as
systemd services that activate **after** boot. They cannot provide secrets
during disk partitioning because:

1. The target root filesystem does not exist yet (it's being created)
2. No systemd services from the target configuration are running
3. The `/run/secrets/` paths these tools create don't exist in the installer

This creates a **bootstrap paradox**: the encrypted disk cannot be created
without the decrypted key, but the key management system cannot run until the
disk exists and the system has booted.

## Solution: nix-secret-bridge

`nix-secret-bridge` is a purpose-built tool that operates in the installer
environment. It:

1. Reads encrypted secret files from your repository (age or SOPS format)
2. Decrypts them using your existing master keys
3. Places the decrypted data on a tmpfs mount (`/run/secrets-bridge/`)
4. Integrates with `disko` via systemd service ordering

After `disko` completes disk formatting, `nix-secret-bridge` securely cleans
up: overwriting secret files with zeros, unlinking them, and unmounting the
tmpfs.

## Basic Usage

### Step 1: Encrypt your LUKS key

```bash
# Generate a random LUKS key
dd if=/dev/urandom of=luks-key bs=4096 count=1

# Encrypt it with age
age -r age1your-public-key... -o secrets/luks-key.age luks-key

# Remove the plaintext key
shred -u luks-key
```

### Step 2: Configure nix-secret-bridge + disko

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    disko.url = "github:nix-community/disko";
    nix-secret-bridge.url = "github:nix-community/nix-secret-bridge";
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

```nix
# configuration.nix
{ config, ... }:
{
  # Configure nix-secret-bridge
  services.nix-secret-bridge = {
    enable = true;
    backend = "age";

    secretMapping = {
      luks-key = {
        encryptedFile = ./secrets/luks-key.age;
        outputPath = "/run/secrets-bridge/luks-key";
      };
    };

    diskoIntegration = true;
  };

  # Configure disko to use the decrypted key
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
        luks = {
          size = "100%";
          content = {
            type = "luks";
            name = "cryptroot";
            settings = {
              keyFile = "/run/secrets-bridge/luks-key";
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

### Step 3: Deploy with nixos-anywhere

```bash
# Provide the age identity for decryption
export NIX_SECRET_BRIDGE_AGE_KEY=$(cat ~/.config/sops/age/keys.txt)

nixos-anywhere --flake .#myhost root@target-machine
```

## Advanced: Using a YubiKey

If your age identity is stored on a YubiKey (via `age-plugin-yubikey`):

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "age";
  secretMapping.luks-key = {
    encryptedFile = ./secrets/luks-key.age;
    outputPath = "/run/secrets-bridge/luks-key";
  };
  # No masterKeyPath needed — the YubiKey plugin handles it
};
```

Then deploy with physical access to the YubiKey:

```bash
nixos-anywhere --flake .#myhost root@target-machine
# You'll be prompted to touch the YubiKey during decryption
```

## Advanced: Using SOPS

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

## Security Considerations

### What IS safe

- **Encrypted files in `/nix/store`**: The `.age` or `.sops` files in your
  repository are encrypted. They can safely exist in the Nix store.
- **The `nix-secret-bridge` binary in `/nix/store`**: It's just a program,
  containing no secrets.

### What is NOT written to disk

- **Decrypted key material**: Only exists on tmpfs and in process memory.
  Never written to any persistent filesystem.
- **Master keys**: Read from environment variables or ephemeral files
  provided via SSH. Never part of a Nix derivation.

### Memory protection

- Process memory is locked with `mlock()` to prevent swapping
- Core dumps are disabled via `prctl(PR_SET_DUMPABLE, 0)`
- All decrypted byte buffers are zeroized on drop (using the `zeroize` crate)
- The Rust binary is compiled with `panic = "abort"` to prevent stack unwinding

### Cleanup

After `disko` finishes:

1. Each secret file is overwritten with zeros
2. Files are unlinked (deleted)
3. The tmpfs is unmounted
4. The mount point directory is removed

This cleanup runs automatically via `nix-secret-bridge-cleanup.service`,
which is ordered after `disko.service`.

## Troubleshooting

### "Master key not found"

Ensure one of these is set:
- `NIX_SECRET_BRIDGE_AGE_KEY` environment variable
- `NIX_SECRET_BRIDGE_MASTER_KEY_FILE` environment variable  
- `services.nix-secret-bridge.provider.masterKeyPath` in your config

### "sops binary not found"

Add `sops` to your installer environment:
```nix
environment.systemPackages = [ pkgs.sops ];
```

### "Failed to mount tmpfs"

The tool requires root privileges. When using `nixos-anywhere`, this is
automatic. For manual installs, run with `sudo`.
