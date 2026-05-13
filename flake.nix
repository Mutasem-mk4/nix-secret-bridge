{
  description = "nix-secret-bridge: Bootstrap secret orchestrator for NixOS disk encryption";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # For integration testing (optional)
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, disko }:
    let
      # The NixOS module — importable by consumers
      nixosModule = import ./module.nix;
    in
    {
      # ── NixOS module outputs ─────────────────────────────────────
      nixosModules.default = nixosModule;
      nixosModules.nix-secret-bridge = nixosModule;

      # ── Overlay ──────────────────────────────────────────────────
      overlays.default = final: prev: {
        nix-secret-bridge = final.callPackage ./package.nix { };
      };

    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ self.overlays.default ];
        };
      in
      {
        # ── Package ────────────────────────────────────────────────
        packages.default = pkgs.nix-secret-bridge;
        packages.nix-secret-bridge = pkgs.nix-secret-bridge;

        # ── Integration tests ──────────────────────────────────────
        checks = let
          testLib = import ./tests {
            inherit pkgs;
            inherit (self) nixosModules;
            diskoModule = disko.nixosModules.disko;
          };
        in {
          inherit (testLib) age-backend-disko sops-backend-disko;
        };

        # ── Dev shell for contributors ─────────────────────────────
        devShells.default = pkgs.mkShell {
          name = "nix-secret-bridge-dev";
          packages = with pkgs; [
            # Rust toolchain
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer

            # Nix tooling
            nixpkgs-fmt
            statix
            deadnix

            # Runtime deps for testing
            age
            sops

            # Build deps
            pkg-config
          ];
          shellHook = ''
            echo "╔══════════════════════════════════════════════╗"
            echo "║  nix-secret-bridge development shell         ║"
            echo "║                                              ║"
            echo "║  cargo build    — build the CLI              ║"
            echo "║  cargo test     — run unit tests             ║"
            echo "║  nix flake check — run integration tests     ║"
            echo "║  nixpkgs-fmt .  — format Nix files           ║"
            echo "╚══════════════════════════════════════════════╝"
          '';
        };
        
        # ── Formatter ──────────────────────────────────────────────
        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
