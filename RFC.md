# RFC: nix-secret-bridge, bootstrap secrets for NixOS disk encryption

## Summary

Add `nix-secret-bridge`, a small Rust CLI and NixOS module that decrypts
bootstrap secrets in the installer environment and exposes them on tmpfs before
`disko.service` runs. This lets `disko` consume LUKS keys during initial disk
formatting without placing plaintext secrets in `/nix/store` or persistent
storage.

## Motivation

NixOS users increasingly deploy encrypted machines with `disko` and
`nixos-anywhere`. The disk formatter runs before the target system exists, while
normal secret managers such as `sops-nix` and `agenix` run after boot. A LUKS
key managed by those tools is therefore unavailable at the exact moment
`cryptsetup luksFormat` needs it.

Current workarounds are unsatisfactory:

- Put the key in Nix, which leaks plaintext into the store.
- Paste the key manually over SSH, which breaks unattended deployment.
- Write one-off shell hooks, which usually miss tmpfs isolation, cleanup,
  ordering, or tests.
- Copy plaintext keys with deployment tooling, which moves the problem outside
  the module system.

The proposed bridge keeps the concerns separated: `disko` remains a disk
declarator, existing age or SOPS files remain the encrypted source of truth, and
the bridge provides only the pre-boot runtime handoff.

## Detailed Design

The module introduces `services.nix-secret-bridge` with these core options:

- `enable`: enable the bridge.
- `backend`: default backend, either `age` or `sops`.
- `secretMapping`: attribute set mapping names to encrypted files and tmpfs
  output paths.
- `masterKeyPath`: runtime path to an age identity file.
- `diskoIntegration`: when true, order the bridge before `disko.service`.

The module also exposes generated paths under
`config.nix-secret-bridge.providers.age.paths` and
`config.nix-secret-bridge.providers.sops.paths`. Disk configurations can refer
to those generated paths instead of duplicating strings.

At runtime:

1. `nix-secret-bridge.service` starts in the installer environment.
2. The service runs `nix-secret-bridge decrypt-all`.
3. The CLI reads encrypted age or SOPS files.
4. The CLI reads the master key from `masterKeyPath`, an environment file, or
   supported environment variables.
5. The CLI calls `mlockall` and disables core dumps on Linux.
6. The CLI mounts `/run/secrets-bridge` as tmpfs with `nodev`, `noexec`,
   `nosuid`, and `noswap` when the kernel supports it.
7. Decrypted bytes are written atomically under the tmpfs mount with mode
   `0400`.
8. `disko.service` runs after the secret is available.
9. `nix-secret-bridge-cleanup.service` overwrites, unlinks, and unmounts the
   tmpfs after `disko.service` completes.

The implementation rejects output paths outside the tmpfs mount. Encrypted
files may be Nix paths because they are ciphertext. Master keys are strings
pointing to runtime paths so Nix does not import plaintext key files into the
store.

## Interaction With Existing Tools

`disko` consumes the generated key file path via its existing `settings.keyFile`
interface. No disk schema changes are required.

`nixos-anywhere` remains responsible for reaching the installer environment.
Users can place the age identity under `/run/nix-secret-bridge` before invoking
deployment or use a runtime systemd environment file.

`agenix` and `sops-nix` remain post-boot secret managers. They can share the
same encrypted source files and age identities, but they do not need to run
during disk formatting.

The SOPS backend shells out to `sops` because SOPS file format support and key
service behavior are already maintained by the upstream binary.

## Drawbacks

The bridge adds another moving part to the installer environment. It also
requires careful documentation because a misconfigured `masterKeyPath` can still
point to a plaintext file that Nix imports into the store. The module mitigates
this by making `masterKeyPath` a string and documenting `/run` as the expected
location.

The first implementation handles age identities and SOPS age keys. Other KMS
backends are intentionally deferred.

## Alternatives

Patch `disko` to decrypt secrets directly. This couples disk declaration to
secret management and expands `disko`'s security surface.

Use ad hoc pre-disko shell scripts. This works for individual users but does not
provide a reusable module, hardened tmpfs handling, cleanup, or VM tests.

Use `nixos-anywhere --extra-files` with plaintext keys. This can be acceptable
for local experiments but does not keep secrets encrypted at rest in the
configuration repository.

Use TPM2 or `systemd-creds` for the first deployment. These are promising for
subsequent boots, but initial provisioning still needs a way to get the first
secret onto the machine before the encrypted root exists.

## Unresolved Questions

- Should TPM2 unsealing be added as a first-class backend after the initial age
  and SOPS implementation is merged?
- Should the official NixOS module expose a higher-level disko helper for common
  encrypted-root layouts, or should it stay as a path provider only?
- Should cleanup default to running even when `disko.service` fails, or should
  operators be able to preserve the tmpfs for emergency debugging?
- Should the module live in nixpkgs immediately or incubate in nix-community
  until more deployment reports are available?

## Implementation Status

The reference implementation includes:

- Rust CLI with age and SOPS backends.
- Linux process hardening through `mlockall` and disabled core dumps.
- tmpfs mounting, atomic file writes, zeroization, and cleanup.
- NixOS module with `Before=disko.service` ordering.
- VM tests for age and SOPS backends.
- Manual documentation and nixos-anywhere examples.

The proposed next step is community review followed by a nixpkgs package PR and
then a NixOS module PR once the interface has maintainer consensus.
