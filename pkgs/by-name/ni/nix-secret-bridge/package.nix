# This is the package expression shape expected for a nixpkgs
# pkgs/by-name/ni/nix-secret-bridge/ PR. In this repository it points at the
# repository root so contributors can evaluate it before copying it into nixpkgs.

{
  lib,
  rustPlatform,
  makeWrapper,
  sops,
}:

rustPlatform.buildRustPackage {
  pname = "nix-secret-bridge";
  version = "0.1.0";

  src = lib.cleanSource ../../../..;

  cargoLock = {
    lockFile = ../../../../Cargo.lock;
  };

  nativeBuildInputs = [
    makeWrapper
  ];

  postInstall = ''
    wrapProgram "$out/bin/nix-secret-bridge" \
      --prefix PATH : ${lib.makeBinPath [ sops ]}
  '';

  meta = {
    description = "Bootstrap secret bridge for NixOS disko LUKS keys";
    homepage = "https://github.com/Mutasem-mk4/nix-secret-bridge";
    license = lib.licenses.mit;
    maintainers = [ ];
    mainProgram = "nix-secret-bridge";
    platforms = lib.platforms.linux;
  };
}
