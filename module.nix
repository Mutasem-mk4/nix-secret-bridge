{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.nix-secret-bridge;

  inherit (lib)
    attrValues
    escapeShellArg
    filterAttrs
    literalExpression
    mapAttrs
    mapAttrs'
    mkEnableOption
    mkIf
    mkMerge
    mkOption
    nameValuePair
    optional
    optionalString
    types
    ;

  secretMappingType = types.submodule {
    options = {
      encryptedFile = mkOption {
        type = types.either types.path types.str;
        description = ''
          Encrypted secret file. Repository-local age and SOPS files may live
          in the Nix store because they contain ciphertext only. Runtime paths
          such as /run/... are also accepted for VM tests and installer hooks.
        '';
        example = literalExpression "./secrets/luks-key.age";
      };

      outputPath = mkOption {
        type = types.str;
        description = ''
          Absolute path under services.nix-secret-bridge.mountBase where the
          decrypted bytes are written on tmpfs.
        '';
        example = "/run/secrets-bridge/luks";
      };

      backend = mkOption {
        type = types.nullOr (types.enum [
          "age"
          "sops"
        ]);
        default = null;
        description = "Optional per-secret backend override.";
      };
    };
  };

  resolveBackend = entry: if entry.backend == null then cfg.backend else entry.backend;

  masterKeyPath =
    if cfg.masterKeyPath != null then cfg.masterKeyPath else cfg.provider.masterKeyPath;

  usesSops =
    cfg.backend == "sops"
    || lib.any (entry: resolveBackend entry == "sops") (attrValues cfg.secretMapping);

  secretMappingJson = pkgs.writeText "nix-secret-bridge-mapping.json" (
    builtins.toJSON (
      mapAttrs (_name: entry: {
        backend = resolveBackend entry;
        encrypted_file = toString entry.encryptedFile;
        output_path = entry.outputPath;
      }) cfg.secretMapping
    )
  );

  decryptCommand =
    ''
      ${cfg.package}/bin/nix-secret-bridge decrypt-all \
        --mapping-file ${escapeShellArg secretMappingJson} \
        --mount-base ${escapeShellArg cfg.mountBase} \
        ${optionalString (masterKeyPath != null) "--master-key-file ${escapeShellArg masterKeyPath}"}
    '';

  providerPathsFor =
    backend:
    mapAttrs' (name: entry: nameValuePair name entry.outputPath) (
      filterAttrs (_name: entry: resolveBackend entry == backend) cfg.secretMapping
    );
in
{
  options = {
    services.nix-secret-bridge = {
      enable = mkEnableOption "pre-disko bootstrap secret decryption";

      package = mkOption {
        type = types.package;
        default = pkgs.nix-secret-bridge;
        defaultText = "pkgs.nix-secret-bridge";
        description = "nix-secret-bridge package to execute.";
      };

      backend = mkOption {
        type = types.enum [
          "age"
          "sops"
        ];
        default = "age";
        description = "Default backend for entries in secretMapping.";
      };

      secretMapping = mkOption {
        type = types.attrsOf secretMappingType;
        default = { };
        description = ''
          Mapping from logical secret names to encrypted sources and tmpfs
          output paths. Decrypted data is never represented in the Nix module
          value graph.
        '';
        example = literalExpression ''
          {
            luks = {
              encryptedFile = ./secrets/luks-key.age;
              outputPath = "/run/secrets-bridge/luks";
            };
          }
        '';
      };

      masterKeyPath = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = ''
          Runtime path to the age identity file used by the age and SOPS
          backends. Use a path under /run for nixos-anywhere deployments.
          Do not point this option at a plaintext key in the source tree.
        '';
        example = "/run/nix-secret-bridge/age-identity.txt";
      };

      provider.masterKeyPath = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = ''
          Deprecated compatibility alias for masterKeyPath. Prefer
          services.nix-secret-bridge.masterKeyPath.
        '';
      };

      environmentFile = mkOption {
        type = types.nullOr types.str;
        default = "/run/nix-secret-bridge/nix-secret-bridge.env";
        description = ''
          Optional runtime EnvironmentFile for the decrypt service. This can
          provide NIX_SECRET_BRIDGE_AGE_KEY, SOPS_AGE_KEY, or SOPS_AGE_KEY_FILE
          without putting secret material in the Nix store.
        '';
      };

      diskoIntegration = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Order nix-secret-bridge.service before disko.service and make disko
          require it. This is the default path for nixos-anywhere.
        '';
      };

      cleanupAfterDisko = mkOption {
        type = types.bool;
        default = true;
        description = "Run nix-secret-bridge-cleanup.service after disko.service.";
      };

      mountBase = mkOption {
        type = types.str;
        default = "/run/secrets-bridge";
        description = "Dedicated tmpfs mount for decrypted bootstrap secrets.";
      };
    };

    nix-secret-bridge.providers = {
      age.paths = mkOption {
        type = types.attrsOf types.str;
        default = { };
        readOnly = true;
        description = "Generated output paths for age-backed secrets.";
      };

      sops.paths = mkOption {
        type = types.attrsOf types.str;
        default = { };
        readOnly = true;
        description = "Generated output paths for SOPS-backed secrets.";
      };
    };
  };

  config = mkIf cfg.enable (mkMerge [
    {
      assertions = [
        {
          assertion = cfg.secretMapping != { };
          message = "services.nix-secret-bridge.enable requires at least one secretMapping entry.";
        }
        {
          assertion = !(cfg.masterKeyPath != null && cfg.provider.masterKeyPath != null);
          message = ''
            Set only services.nix-secret-bridge.masterKeyPath. The nested
            provider.masterKeyPath alias is retained only for compatibility.
          '';
        }
        {
          assertion = lib.all (
            entry:
            lib.hasPrefix "${cfg.mountBase}/" entry.outputPath
          ) (attrValues cfg.secretMapping);
          message = ''
            Every services.nix-secret-bridge.secretMapping.*.outputPath must
            be under services.nix-secret-bridge.mountBase (${cfg.mountBase}).
          '';
        }
        {
          assertion =
            masterKeyPath == null
            || !(lib.hasPrefix builtins.storeDir (toString masterKeyPath));
          message = ''
            services.nix-secret-bridge.masterKeyPath must be a runtime path,
            not a Nix store path. Put the age identity under /run and pass that
            path instead.
          '';
        }
      ];

      environment.systemPackages = [
        cfg.package
      ] ++ optional usesSops pkgs.sops;

      nix-secret-bridge.providers.age.paths = providerPathsFor "age";
      nix-secret-bridge.providers.sops.paths = providerPathsFor "sops";

      system.activationScripts.nix-secret-bridge-validate = ''
        ${cfg.package}/bin/nix-secret-bridge validate-mapping \
          --mapping-file ${escapeShellArg secretMappingJson} \
          --mount-base ${escapeShellArg cfg.mountBase} \
          --allow-missing-inputs
      '';

      systemd.mounts = [
        {
          description = "tmpfs for nix-secret-bridge decrypted bootstrap secrets";
          what = "tmpfs";
          where = cfg.mountBase;
          type = "tmpfs";
          options = "mode=0700,size=1m,noswap,nodev,noexec,nosuid";
          before = [ "nix-secret-bridge.service" ];
          requiredBy = [ "nix-secret-bridge.service" ];
        }
      ];

      systemd.services.nix-secret-bridge = {
        description = "Decrypt bootstrap secrets for disko";
        documentation = [ "file:${./doc/manual.md}" ];
        wantedBy = mkIf (!cfg.diskoIntegration) [ "multi-user.target" ];
        before = mkIf cfg.diskoIntegration [ "disko.service" ];
        requiredBy = mkIf cfg.diskoIntegration [ "disko.service" ];
        path = [ cfg.package ] ++ optional usesSops pkgs.sops;
        script = decryptCommand;
        environment = {
          RUST_LOG = "warn";
        };

        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          EnvironmentFile = optional (cfg.environmentFile != null) "-${cfg.environmentFile}";
          StandardOutput = "null";
          StandardError = "journal";
          UMask = "0077";
          ReadWritePaths = [
            "/run"
            cfg.mountBase
          ];
          CapabilityBoundingSet = [
            "CAP_DAC_OVERRIDE"
            "CAP_FOWNER"
            "CAP_IPC_LOCK"
          ];
          AmbientCapabilities = [ "CAP_IPC_LOCK" ];
          NoNewPrivileges = false;
          PrivateTmp = true;
          ProtectHome = true;
          ProtectSystem = "strict";
          ProtectKernelTunables = true;
          ProtectKernelModules = true;
          ProtectControlGroups = true;
          RestrictSUIDSGID = true;
          MemoryDenyWriteExecute = true;
          LimitMEMLOCK = "infinity";
        };
      };
    }

    (mkIf (cfg.cleanupAfterDisko && cfg.diskoIntegration) {
      systemd.services.nix-secret-bridge-cleanup = {
        description = "Clean up bootstrap secrets after disko";
        wantedBy = [ "disko.service" ];
        after = [ "disko.service" ];
        path = [ cfg.package ];
        script = ''
          ${cfg.package}/bin/nix-secret-bridge cleanup --mount-base ${escapeShellArg cfg.mountBase}
        '';
        serviceConfig = {
          Type = "oneshot";
          StandardOutput = "null";
          StandardError = "journal";
          UMask = "0077";
          ReadWritePaths = [
            "/run"
            cfg.mountBase
          ];
          CapabilityBoundingSet = [
            "CAP_SYS_ADMIN"
            "CAP_DAC_OVERRIDE"
            "CAP_FOWNER"
          ];
          AmbientCapabilities = [ "CAP_SYS_ADMIN" ];
          NoNewPrivileges = false;
          ProtectSystem = "strict";
          ProtectHome = true;
        };
      };
    })
  ]);
}
