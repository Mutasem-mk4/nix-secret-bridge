# nix-secret-bridge

`nix-secret-bridge` provides encrypted bootstrap secrets to `disko` before the
target NixOS system has booted. Its primary use case is an encrypted disk whose
LUKS key is itself stored as an encrypted repository secret.

## Bootstrap Paradox

`disko` formats disks in the installer environment used by a live ISO or
`nixos-anywhere`. Secret managers such as `sops-nix` and `agenix` normally run
as services in the installed target system. That creates a timing problem:
`disko` needs the LUKS key during `luksFormat`, but the usual secret service
cannot run until after the encrypted root filesystem exists and has booted.

`nix-secret-bridge` solves this by running in the installer environment. It
decrypts an age or SOPS file in memory, mounts a dedicated tmpfs at
`/run/secrets-bridge`, writes the plaintext key there with mode `0400`, and
orders itself before `disko.service`. The cleanup service overwrites and unlinks
the tmpfs files after `disko` finishes.

## Installation

From a flake:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    disko.url = "github:nix-community/disko";
    nix-secret-bridge.url = "github:Mutasem-mk4/nix-secret-bridge";
  };

  outputs = { nixpkgs, disko, nix-secret-bridge, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
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

After inclusion in `nixpkgs`, use the NixOS module from the normal module set
and `pkgs.nix-secret-bridge` as the package.

## Basic Configuration

```nix
{ config, ... }:

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

  disko.devices.disk.main = {
    type = "disk";
    device = "/dev/sda";
    content = {
      type = "gpt";
      partitions.root = {
        size = "100%";
        content = {
          type = "luks";
          name = "cryptroot";
          settings.keyFile = config.nix-secret-bridge.providers.age.paths.luks;
          content = {
            type = "filesystem";
            format = "ext4";
            mountpoint = "/";
          };
        };
      };
    };
  };
}
```

The encrypted `luks-key.age` file may be copied into the Nix store. The
plaintext age identity must not be stored in the source tree or imported as a
Nix path. Place it under `/run` on the installer host before running
`nixos-anywhere`.

## Deploying with nixos-anywhere

```bash
export NIX_SECRET_BRIDGE_AGE_KEY="$(cat ~/.config/sops/age/keys.txt)"
./examples/deploy.sh .#host root@192.0.2.10
```

The example script transfers the local environment variable to
`/run/nix-secret-bridge/age-identity.txt` on the target over SSH, runs
`nixos-anywhere`, and removes the remote key file afterward.

## SOPS Backend

For SOPS, use a SOPS binary file for the LUKS key and set the backend:

```nix
services.nix-secret-bridge = {
  enable = true;
  backend = "sops";
  masterKeyPath = "/run/nix-secret-bridge/age-identity.txt";
  secretMapping.luks = {
    encryptedFile = ./secrets/luks-key.sops;
    outputPath = "/run/secrets-bridge/luks";
  };
};
```

The SOPS backend invokes `sops --decrypt --output-type binary`. The module adds
`pkgs.sops` to the installer environment when any mapping uses the SOPS backend.

## Advanced Key Sources

The tested key-source paths are a runtime age identity file under `/run` and
runtime environment variables passed through the deployment wrapper. Do not
import plaintext identity files as Nix paths.

Hardware-backed age plugin identities and SSH age identities are intentionally
not part of the initial native age backend. They add dependency surface and
should only be enabled after automated tests cover unattended installer
execution.

TPM2 sealing is intentionally not part of the first implementation. TPM2 is
better suited for subsequent boots or redeployments after the target hardware
has established sealed credentials. A future module can add a TPM2 provider
without changing the tmpfs handoff contract used by `disko`.

## Options

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `services.nix-secret-bridge.enable` | boolean | `false` | Enables the bridge service. |
| `services.nix-secret-bridge.package` | package | `pkgs.nix-secret-bridge` | Package providing the CLI. |
| `services.nix-secret-bridge.backend` | `"age"` or `"sops"` | `"age"` | Default backend for mappings. |
| `services.nix-secret-bridge.secretMapping` | attribute set | `{}` | Maps secret names to encrypted files and tmpfs output paths. |
| `services.nix-secret-bridge.secretMapping.<name>.encryptedFile` | path or string | required | Encrypted age or SOPS source. |
| `services.nix-secret-bridge.secretMapping.<name>.outputPath` | string | required | Plaintext tmpfs path under `mountBase`. |
| `services.nix-secret-bridge.secretMapping.<name>.backend` | null or enum | `null` | Per-secret backend override. |
| `services.nix-secret-bridge.masterKeyPath` | null or string | `null` | Runtime path to the age identity file. |
| `services.nix-secret-bridge.environmentFile` | null or string | `/run/nix-secret-bridge/nix-secret-bridge.env` | Optional systemd environment file for key variables. |
| `services.nix-secret-bridge.diskoIntegration` | boolean | `true` | Adds `Before=disko.service` and `RequiredBy=disko.service`. |
| `services.nix-secret-bridge.cleanupAfterDisko` | boolean | `true` | Starts cleanup after `disko.service`. |
| `services.nix-secret-bridge.mountBase` | string | `/run/secrets-bridge` | Dedicated tmpfs mount for plaintext secrets. |
| `config.nix-secret-bridge.providers.age.paths` | attribute set | generated | Output paths for age-backed mappings. |
| `config.nix-secret-bridge.providers.sops.paths` | attribute set | generated | Output paths for SOPS-backed mappings. |

## Security Considerations

Plaintext bootstrap secrets are runtime data. Do not put them in Nix values,
derivations, flake inputs, or files that Nix will copy to `/nix/store`.

The bridge enforces these properties:

- Decrypted data is written only under the configured tmpfs mount.
- Output paths outside `mountBase` are rejected by the CLI and module.
- Secret files are created atomically with parent directories mode `0700` and
  files mode `0400`.
- Replaced and cleaned-up files are overwritten with zeros before unlinking.
- Decrypted buffers use `zeroize::Zeroizing`.
- The process disables core dumps and calls `mlockall` on Linux.
- The systemd unit suppresses stdout and only logs sanitized errors.

Encrypted files may be stored in the Nix store. Master keys and decrypted LUKS
keys must be delivered at runtime through `/run`, SSH transport, or another
deployment-time secret transfer mechanism.
