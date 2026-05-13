{
  config,
  pkgs,
  ...
}:

{
  imports = [
    ./disko-config.nix
  ];

  services.openssh.enable = true;

  environment.systemPackages = with pkgs; [
    age
    cryptsetup
    disko
    nixos-anywhere
    sops
  ];

  services.nix-secret-bridge = {
    enable = true;
    backend = "age";
    masterKeyPath = "/run/nix-secret-bridge/age-identity.txt";
    diskoIntegration = true;
    cleanupAfterDisko = true;

    secretMapping.luks = {
      encryptedFile = ./secrets/luks-key.age;
      outputPath = "/run/secrets-bridge/luks";
    };
  };

  boot.loader.grub.enable = false;
  system.stateVersion = "25.11";
}
