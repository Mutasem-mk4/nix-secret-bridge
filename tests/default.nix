# Integration test suite for nix-secret-bridge
#
# Provides two NixOS VM tests:
#   1. age-backend-disko — Age backend with disko LUKS encryption
#   2. sops-backend-disko — SOPS backend with disko LUKS encryption
#
# Each test verifies:
#   - LUKS partition is properly formatted and unlockable
#   - Decrypted secret is NOT present in /nix/store
#   - tmpfs secret is removed after disko finishes
#   - Correct file permissions on the tmpfs secret (during its lifetime)

{ pkgs, nixosModules, diskoModule }:

let
  inherit (pkgs) lib;

  # Generate a test age keypair deterministically for reproducible tests
  # In real tests, we'd use `age-keygen` but for Nix purity we pre-generate
  testAgePublicKey = "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p";

  # Create a test age identity file (this is a FAKE key for testing only)
  # In real CI, this would be generated fresh via `age-keygen`
  testAgeIdentity = pkgs.writeText "test-age-identity" ''
    # created: 2026-01-01T00:00:00Z
    # public key: ${testAgePublicKey}
    AGE-SECRET-KEY-1QFGCE9G78TWNZ6YQ5PSDXLNMJEZF3FWZGTXEMYZAC73HZMXE4LQAREFKF
  '';

  # Create a test LUKS key and encrypt it with age
  testLuksKey = "test-luks-passphrase-for-ci-only";

  # Script to generate the encrypted test secret
  generateTestSecret = pkgs.writeShellScript "gen-test-secret" ''
    echo -n "${testLuksKey}" | ${pkgs.age}/bin/age -r "${testAgePublicKey}" -o "$1"
  '';

  # Generate SOPS-encrypted test secret
  generateSopsSecret = pkgs.writeShellScript "gen-sops-secret" ''
    export SOPS_AGE_KEY_FILE=${testAgeIdentity}
    echo -n "${testLuksKey}" > /tmp/luks-key-plain
    ${pkgs.sops}/bin/sops --encrypt \
      --age "${testAgePublicKey}" \
      --input-type binary \
      --output-type binary \
      /tmp/luks-key-plain > "$1"
    rm -f /tmp/luks-key-plain
  '';

  # Common test machine config
  commonTestConfig = { config, ... }: {
    imports = [
      nixosModules.default
      diskoModule
    ];

    # Virtual disk for testing
    virtualisation = {
      emptyDiskImages = [ 1024 ]; # 1GB test disk
      memorySize = 2048;
    };

    # Basic system config
    boot.loader.grub.enable = false;
    system.stateVersion = "24.11";
  };

in
{
  # ──────────────────────────────────────────────────────────────────
  # TEST 1: Age backend + disko LUKS encryption
  # ──────────────────────────────────────────────────────────────────
  age-backend-disko = pkgs.testers.nixosTest {
    name = "nix-secret-bridge-age-backend";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ commonTestConfig ];

      # Prepare the encrypted secret in the test environment
      system.activationScripts.prepareTestSecret = ''
        ${generateTestSecret} /tmp/luks-key.age
      '';

      # Configure nix-secret-bridge
      services.nix-secret-bridge = {
        enable = true;
        backend = "age";

        secretMapping = {
          luks-key = {
            encryptedFile = "/tmp/luks-key.age";
            outputPath = "/run/secrets-bridge/luks-key";
          };
        };

        provider.masterKeyPath = testAgeIdentity;
        diskoIntegration = false;  # We'll trigger manually in the test
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Verify the encrypted file was created
      machine.succeed("test -f /tmp/luks-key.age")

      # Run nix-secret-bridge decrypt
      machine.succeed(
          "nix-secret-bridge decrypt "
          "--backend age "
          "--encrypted-file /tmp/luks-key.age "
          "--output-path /run/secrets-bridge/luks-key "
          "--master-key-file ${testAgeIdentity}"
      )

      # Verify the decrypted secret exists on tmpfs
      machine.succeed("test -f /run/secrets-bridge/luks-key")

      # Verify correct permissions (0400)
      machine.succeed(
          "stat -c '%a' /run/secrets-bridge/luks-key | grep -q '400'"
      )

      # Verify the decrypted content is correct
      machine.succeed(
          "cat /run/secrets-bridge/luks-key | grep -q '${testLuksKey}'"
      )

      # Verify the mount is tmpfs
      machine.succeed(
          "mount | grep '/run/secrets-bridge' | grep -q 'tmpfs'"
      )

      # Verify the secret is NOT in /nix/store (only encrypted version may be)
      machine.fail(
          "grep -r '${testLuksKey}' /nix/store/ 2>/dev/null"
      )

      # Use the secret with cryptsetup (simulating disko's luksFormat)
      machine.succeed(
          "cryptsetup luksFormat --batch-mode "
          "--key-file /run/secrets-bridge/luks-key "
          "/dev/vdb"
      )

      # Verify the LUKS partition was created
      machine.succeed("cryptsetup isLuks /dev/vdb")

      # Open the LUKS partition to verify the key works
      machine.succeed(
          "cryptsetup luksOpen "
          "--key-file /run/secrets-bridge/luks-key "
          "/dev/vdb crypttest"
      )
      machine.succeed("test -b /dev/mapper/crypttest")
      machine.succeed("cryptsetup close crypttest")

      # Run cleanup
      machine.succeed("nix-secret-bridge cleanup --mount-base /run/secrets-bridge")

      # Verify cleanup: secret file is gone
      machine.fail("test -f /run/secrets-bridge/luks-key")

      # Verify cleanup: tmpfs is unmounted
      machine.fail("mount | grep '/run/secrets-bridge'")
    '';
  };

  # ──────────────────────────────────────────────────────────────────
  # TEST 2: SOPS backend + disko LUKS encryption
  # ──────────────────────────────────────────────────────────────────
  sops-backend-disko = pkgs.testers.nixosTest {
    name = "nix-secret-bridge-sops-backend";

    nodes.machine = { config, pkgs, ... }: {
      imports = [ commonTestConfig ];

      environment.systemPackages = [ pkgs.sops pkgs.age ];

      # Prepare SOPS-encrypted secret
      system.activationScripts.prepareSopsSecret = ''
        ${generateSopsSecret} /tmp/luks-key.sops
      '';

      services.nix-secret-bridge = {
        enable = true;
        backend = "sops";

        secretMapping = {
          luks-key = {
            encryptedFile = "/tmp/luks-key.sops";
            outputPath = "/run/secrets-bridge/luks-key";
          };
        };

        provider.masterKeyPath = testAgeIdentity;
        diskoIntegration = false;
      };
    };

    testScript = ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      # Verify SOPS encrypted file exists
      machine.succeed("test -f /tmp/luks-key.sops")

      # Run nix-secret-bridge decrypt with SOPS backend
      machine.succeed(
          "nix-secret-bridge decrypt "
          "--backend sops "
          "--encrypted-file /tmp/luks-key.sops "
          "--output-path /run/secrets-bridge/luks-key "
          "--master-key-file ${testAgeIdentity}"
      )

      # Verify decrypted secret
      machine.succeed("test -f /run/secrets-bridge/luks-key")
      machine.succeed("stat -c '%a' /run/secrets-bridge/luks-key | grep -q '400'")

      # Verify NOT in /nix/store
      machine.fail("grep -r '${testLuksKey}' /nix/store/ 2>/dev/null")

      # Use with cryptsetup
      machine.succeed(
          "cryptsetup luksFormat --batch-mode "
          "--key-file /run/secrets-bridge/luks-key "
          "/dev/vdb"
      )
      machine.succeed("cryptsetup isLuks /dev/vdb")

      # Cleanup
      machine.succeed("nix-secret-bridge cleanup --mount-base /run/secrets-bridge")
      machine.fail("test -f /run/secrets-bridge/luks-key")
      machine.fail("mount | grep '/run/secrets-bridge'")
    '';
  };
}
