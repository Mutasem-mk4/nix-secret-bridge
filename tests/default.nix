{
  pkgs,
  nixosModules,
  diskoModule,
  diskoPackage,
}:

let
  lib = pkgs.lib;

  testAgePublicKey = "age1huksd8cy83aqsw7mqq3an4kq2zzz5rxfn6pmd75j4dea6an6lagq4dxaxz";

  testAgeIdentity = pkgs.writeText "nix-secret-bridge-test-age-identity.txt" ''
    # public key: ${testAgePublicKey}
    AGE-SECRET-KEY-1QHKJV8MW7HE0C40RUT8WKE5ZQ9JVA9RJ675E37ZK7TQZP9MXMTNSSAVJH7
  '';

  runtimeDir = "/run/nix-secret-bridge-test";
  runtimeIdentity = "${runtimeDir}/age-identity.txt";
  bridgePath = "/run/secrets-bridge/luks";

  prepareAgeSecret = pkgs.writeShellScript "prepare-age-secret" ''
    set -eu
    install -m 0700 -d ${runtimeDir}
    ${pkgs.openssl}/bin/openssl rand -hex 64 | ${pkgs.coreutils}/bin/head -c 128 > ${runtimeDir}/plain
    ${pkgs.age}/bin/age --encrypt \
      --recipient ${lib.escapeShellArg testAgePublicKey} \
      --output ${runtimeDir}/luks-key.age \
      ${runtimeDir}/plain
  '';

  prepareSopsSecret = pkgs.writeShellScript "prepare-sops-secret" ''
    set -eu
    install -m 0700 -d ${runtimeDir}
    ${pkgs.openssl}/bin/openssl rand -hex 64 | ${pkgs.coreutils}/bin/head -c 128 > ${runtimeDir}/plain
    ${pkgs.sops}/bin/sops --encrypt \
      --age ${lib.escapeShellArg testAgePublicKey} \
      --input-type binary \
      --output-type binary \
      ${runtimeDir}/plain > ${runtimeDir}/luks-key.sops
  '';

  commonNode =
    {
      config,
      pkgs,
      ...
    }:
    {
      imports = [
        nixosModules.default
        diskoModule
      ];

      virtualisation.emptyDiskImages = [ 2048 ];
      virtualisation.memorySize = 2048;

      boot.loader.grub.enable = false;
      system.stateVersion = "25.11";

      environment.systemPackages = [
        config.services.nix-secret-bridge.package
        diskoPackage
        pkgs.age
        pkgs.cryptsetup
        pkgs.e2fsprogs
        pkgs.openssl
        pkgs.sops
        pkgs.util-linux
      ];

      services.nix-secret-bridge = {
        enable = true;
        masterKeyPath = runtimeIdentity;
        diskoIntegration = true;
        cleanupAfterDisko = false;
        secretMapping.luks.outputPath = bridgePath;
      };

      systemd.services.prepare-runtime-identity = {
        description = "Copy test age identity into /run";
        before = [ "nix-secret-bridge.service" ];
        requiredBy = [ "nix-secret-bridge.service" ];
        serviceConfig.Type = "oneshot";
        script = ''
          install -m 0700 -d ${runtimeDir}
          install -m 0600 ${testAgeIdentity} ${runtimeIdentity}
        '';
      };

      disko.devices.disk.test = {
        type = "disk";
        device = "/dev/vdb";
        content = {
          type = "gpt";
          partitions.root = {
            size = "100%";
            content = {
              type = "luks";
              name = "crypttest";
              settings = {
                keyFile = bridgePath;
                keyFileSize = 128;
              };
              content = {
                type = "filesystem";
                format = "ext4";
                mountpoint = "/mnt/crypttest";
              };
            };
          };
        };
      };

      systemd.services.disko = {
        description = "Run disko for nix-secret-bridge integration test";
        wantedBy = [ "multi-user.target" ];
        after = [ "nix-secret-bridge.service" ];
        requires = [ "nix-secret-bridge.service" ];
        path = [
          diskoPackage
          pkgs.cryptsetup
          pkgs.e2fsprogs
          pkgs.util-linux
        ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          ${config.system.build.diskoScript}
        '';
      };
    };

  testScript = ''
    def dump_bridge_diagnostics():
        machine.succeed("systemctl status nix-secret-bridge.service --no-pager || true")
        machine.succeed("journalctl -u nix-secret-bridge.service --no-pager || true")
        machine.succeed("systemctl status disko.service --no-pager || true")
        machine.succeed("journalctl -u disko.service --no-pager || true")
        machine.succeed("findmnt /run/secrets-bridge || true")
        machine.succeed("ls -la /run/secrets-bridge || true")

    def wait_for_unit_or_dump(unit):
        try:
            machine.wait_for_unit(unit)
        except Exception:
            dump_bridge_diagnostics()
            raise

    machine.start()
    wait_for_unit_or_dump("multi-user.target")
    wait_for_unit_or_dump("nix-secret-bridge.service")
    wait_for_unit_or_dump("disko.service")

    machine.succeed("test -f ${bridgePath}")
    machine.succeed("stat -c '%a' ${bridgePath} | grep -qx 400")
    machine.succeed("findmnt -n -t tmpfs /run/secrets-bridge")
    machine.succeed("cmp -s ${runtimeDir}/plain ${bridgePath}")

    machine.succeed("cryptsetup isLuks /dev/vdb1")
    machine.succeed("if [ ! -b /dev/mapper/crypttest ]; then cryptsetup open --key-file ${bridgePath} /dev/vdb1 crypttest; fi; test -b /dev/mapper/crypttest")
    machine.succeed("if grep -R -F -- \"$(cat ${runtimeDir}/plain)\" /nix/store >/tmp/store-grep.log 2>&1; then exit 1; else test $? -eq 1; fi")

    machine.succeed("nix-secret-bridge cleanup --mount-base /run/secrets-bridge")
    machine.fail("test -e ${bridgePath}")
    machine.fail("findmnt /run/secrets-bridge")
  '';

  age-backend-disko = pkgs.testers.nixosTest {
    name = "nix-secret-bridge-age-backend-disko";
    nodes.machine =
      { ... }:
      {
        imports = [ commonNode ];

        systemd.services.prepare-age-secret = {
          description = "Create runtime age-encrypted LUKS key";
          before = [ "nix-secret-bridge.service" ];
          requiredBy = [ "nix-secret-bridge.service" ];
          serviceConfig.Type = "oneshot";
          script = ''
            ${prepareAgeSecret}
          '';
        };

        services.nix-secret-bridge = {
          backend = "age";
          secretMapping.luks.encryptedFile = "${runtimeDir}/luks-key.age";
        };
      };
    inherit testScript;
  };

  sops-backend-disko = pkgs.testers.nixosTest {
    name = "nix-secret-bridge-sops-backend-disko";
    nodes.machine =
      { ... }:
      {
        imports = [ commonNode ];

        systemd.services.prepare-sops-secret = {
          description = "Create runtime SOPS-encrypted LUKS key";
          before = [ "nix-secret-bridge.service" ];
          requiredBy = [ "nix-secret-bridge.service" ];
          serviceConfig.Type = "oneshot";
          script = ''
            ${prepareSopsSecret}
          '';
        };

        services.nix-secret-bridge = {
          backend = "sops";
          secretMapping.luks.encryptedFile = "${runtimeDir}/luks-key.sops";
        };
      };
    inherit testScript;
  };
in
{
  inherit age-backend-disko sops-backend-disko;

  integration = pkgs.releaseTools.aggregate {
    name = "nix-secret-bridge-integration";
    constituents = [
      age-backend-disko
      sops-backend-disko
    ];
  };
}
