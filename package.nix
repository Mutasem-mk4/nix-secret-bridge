# Nix package expression for nix-secret-bridge
#
# Builds the Rust CLI binary using rustPlatform.buildRustPackage.
# For inclusion in nixpkgs via pkgs/by-name/ni/nix-secret-bridge/package.nix

{ lib
, rustPlatform
, pkg-config
, installShellFiles
, sops
}:

rustPlatform.buildRustPackage {
  pname = "nix-secret-bridge";
  version = "0.1.0";

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
    # If using git dependencies, allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    pkg-config
    installShellFiles
  ];

  # sops is needed at runtime for the SOPS backend
  propagatedBuildInputs = [
    sops
  ];

  # Disable tests that require root (mount operations)
  checkFlags = [
    "--skip=mount"
    "--skip=cleanup"
  ];

  postInstall = ''
    # Install shell completions
    installShellCompletion --cmd nix-secret-bridge \
      --bash <($out/bin/nix-secret-bridge completions bash 2>/dev/null || true) \
      --zsh <($out/bin/nix-secret-bridge completions zsh 2>/dev/null || true) \
      --fish <($out/bin/nix-secret-bridge completions fish 2>/dev/null || true)
  '';

  meta = with lib; {
    description = "Bootstrap secret orchestrator for NixOS disk encryption";
    longDescription = ''
      nix-secret-bridge solves the bootstrap paradox for NixOS deployments:
      it decrypts secrets (LUKS passwords, disk encryption keys) during the
      initial disk partitioning phase, before the target system is booted,
      so that disko can consume them.

      Supports age (agenix-compatible) and SOPS (sops-nix-compatible) backends.
      Decrypted secrets exist only on tmpfs and are automatically cleaned up.
    '';
    homepage = "https://github.com/nix-community/nix-secret-bridge";
    license = licenses.mit;
    maintainers = [ ];  # Add maintainer entries
    platforms = platforms.linux;
    mainProgram = "nix-secret-bridge";
  };
}
