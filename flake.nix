{
  description = "Bootstrap secret bridge for NixOS disko LUKS keys";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      disko,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      nixosModule = import ./module.nix;
    in
    {
      nixosModules.default = nixosModule;
      nixosModules.nix-secret-bridge = nixosModule;

      overlays.default = final: _prev: {
        nix-secret-bridge = final.callPackage ./package.nix { };
      };
    }
    // flake-utils.lib.eachSystem supportedSystems (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ self.overlays.default ];
        };
        integrationTests = import ./tests {
          inherit pkgs;
          nixosModules = self.nixosModules;
          diskoModule = disko.nixosModules.disko;
          diskoPackage = disko.packages.${system}.disko;
        };
      in
      {
        packages.default = pkgs.nix-secret-bridge;
        packages.nix-secret-bridge = pkgs.nix-secret-bridge;

        checks = {
          rust-test = pkgs.nix-secret-bridge;
          integration = integrationTests.integration;
          age-backend-disko = integrationTests.age-backend-disko;
          sops-backend-disko = integrationTests.sops-backend-disko;
        };

        formatter = pkgs.nixfmt-rfc-style;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            age
            cargo
            clippy
            cryptsetup
            deadnix
            nixfmt-rfc-style
            rust-analyzer
            rustc
            rustfmt
            sops
            statix
          ];
        };
      }
    );
}
