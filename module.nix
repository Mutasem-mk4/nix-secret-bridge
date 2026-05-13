# NixOS Module: nix-secret-bridge
#
# Provides bootstrap secret decryption for disko's disk encryption
# phase. Runs in the installer environment BEFORE the target system
# is booted, bridging the gap between encrypted secret files and
# tools that need plaintext keys at format time.
#
# Security invariants:
#   - Decrypted secrets exist ONLY on tmpfs (never /nix/store)
#   - In-memory buffers are zeroized after use
#   - tmpfs is mounted with noexec,nosuid,nodev,noswap
#   - All secret files are mode 0400, owned by root
#   - Cleanup service removes all traces after disko finishes

{ config, lib, pkgs, ... }:

let
  cfg = config.services.nix-secret-bridge;
  inherit (lib) mkEnableOption mkOption mkIf mkMerge types;

  # Type for a single secret mapping entry
  secretMappingType = types.submodule {
    options = {
      encryptedFile = mkOption {
        type = types.path;
        description = ''
          Path to the encrypted secret file (age-encrypted or SOPS-encrypted).
          This file CAN be in the Nix store (it's encrypted, so that's safe).
        '';
        example = "./secrets/luks-key.age";
      };

      outputPath = mkOption {
        type = types.str;
        description = ''
          Absolute path where the decrypted secret will be written on tmpfs.
          This path will be consumed by disko or other tools.
        '';
        example = "/run/secrets-bridge/luks-key";
      };

      backend = mkOption {
        type = types.nullOr (types.enum [ "age" "sops" ]);
        default = null;
        description = ''
          Override the global backend for this specific secret.
          If null, uses the global `services.nix-secret-bridge.backend` setting.
        '';
      };
    };
  };

  # Resolve the backend for a given secret entry
  resolveBackend = entry:
    if entry.backend != null then entry.backend else cfg.backend;

  # Generate the JSON mapping file consumed by the CLI's `decrypt-all`
  secretMappingJson = pkgs.writeText "nix-secret-bridge-mapping.json"
    (builtins.toJSON (lib.mapAttrs (name: entry: {
      backend = resolveBackend entry;
      encrypted_file = toString entry.encryptedFile;
      output_path = entry.outputPath;
    }) cfg.secretMapping));

  # The nix-secret-bridge binary package
  bridgePkg = cfg.package;

in
{
  # ────────────────────────────────────────────────────────────────
  # OPTIONS
  # ────────────────────────────────────────────────────────────────

  options.services.nix-secret-bridge = {
    enable = mkEnableOption "nix-secret-bridge bootstrap secret orchestrator";

    package = mkOption {
      type = types.package;
      default = pkgs.nix-secret-bridge;
      defaultText = "pkgs.nix-secret-bridge";
      description = "The nix-secret-bridge package to use.";
    };

    backend = mkOption {
      type = types.enum [ "age" "sops" ];
      default = "age";
      description = ''
        Default decryption backend. Can be overridden per-secret in
        `secretMapping.<name>.backend`.

        - `age`: Uses the rage library (compatible with agenix)
        - `sops`: Uses the sops CLI (compatible with sops-nix)
      '';
    };

    secretMapping = mkOption {
      type = types.attrsOf secretMappingType;
      default = { };
      description = ''
        Mapping of secret names to their encrypted sources and output paths.
        Each entry specifies an encrypted file and where the decrypted
        result should be placed on tmpfs.
      '';
      example = lib.literalExpression ''
        {
          luks-key = {
            encryptedFile = ./secrets/luks-key.age;
            outputPath = "/run/secrets-bridge/luks-key";
          };
          luks-data-key = {
            encryptedFile = ./secrets/data-key.age;
            outputPath = "/run/secrets-bridge/data-key";
          };
        }
      '';
    };

    provider = {
      masterKeyPath = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = ''
          Path to the master key file for non-interactive use.
          If null, the tool will look for:
          - NIX_SECRET_BRIDGE_AGE_KEY environment variable
          - NIX_SECRET_BRIDGE_MASTER_KEY_FILE environment variable
          - SOPS_AGE_KEY_FILE environment variable
        '';
        example = "/tmp/age-identity.txt";
      };
    };

    diskoIntegration = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Automatically hook into disko's pre-format phase via systemd
        service ordering. When true, nix-secret-bridge-installer.service
        will be ordered Before=disko.service and cleanup will run after.
      '';
    };

    mountBase = mkOption {
      type = types.str;
      default = "/run/secrets-bridge";
      description = ''
        Base directory for the tmpfs mount where decrypted secrets
        are placed. This directory is created and mounted automatically.
      '';
    };

    cleanupAfterDisko = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Whether to automatically clean up (zeroize + unmount) decrypted
        secrets after disko finishes. Disable only if other services
        need the secrets after partitioning.
      '';
    };
  };

  # ────────────────────────────────────────────────────────────────
  # IMPLEMENTATION
  # ────────────────────────────────────────────────────────────────

  config = mkIf cfg.enable (mkMerge [
    # ── Core: decrypt service ────────────────────────────────────
    {
      assertions = [
        {
          assertion = cfg.secretMapping != { };
          message = ''
            services.nix-secret-bridge is enabled but no secrets are
            configured. Add entries to services.nix-secret-bridge.secretMapping.
          '';
        }
        {
          assertion = cfg.provider.masterKeyPath == null || !(lib.hasPrefix builtins.storeDir (toString cfg.provider.masterKeyPath));
          message = ''
            nix-secret-bridge: `masterKeyPath` must not be a file in the Nix store!
            Putting your master key in the Nix store exposes it to all local users
            and breaks the confidentiality of the entire secret bridging process.
          '';
        }
      ];

      # Ensure required packages are available
      environment.systemPackages = [ bridgePkg ]
        ++ lib.optional (cfg.backend == "sops" ||
            lib.any (e: resolveBackend e == "sops") (lib.attrValues cfg.secretMapping))
          pkgs.sops;

      # Main decryption service — runs in the installer environment
      systemd.services.nix-secret-bridge-installer = {
        description = "nix-secret-bridge: Decrypt bootstrap secrets for disk encryption";
        wantedBy = [ "multi-user.target" ];

        # Security: suppress all output to prevent secret leakage
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;

          LoadCredential = lib.optional (cfg.provider.masterKeyPath != null)
            "master_key:${toString cfg.provider.masterKeyPath}";

          ExecStart = let
            masterKeyArg = lib.optionalString (cfg.provider.masterKeyPath != null)
              "--master-key-file %d/master_key";
          in
            "${bridgePkg}/bin/nix-secret-bridge decrypt-all "
            + "--mapping-file ${secretMappingJson} "
            + "--mount-base ${cfg.mountBase} "
            + masterKeyArg;

          # Hardening
          StandardOutput = "null";
          StandardError = "journal";
          PrivateTmp = true;
          ProtectSystem = "strict";
          ReadWritePaths = [ cfg.mountBase "/run" ];
          NoNewPrivileges = false; # Need mount capability
          ProtectHome = true;
          ProtectKernelTunables = true;
          ProtectKernelModules = true;
          ProtectControlGroups = true;
          RestrictSUIDSGID = true;
          MemoryDenyWriteExecute = true;
          LimitMEMLOCK = "infinity"; # Allow memory locking for secure buffer wiping
        };

        unitConfig = {
          # Don't retry on failure — fail loudly so the user knows
          StartLimitIntervalSec = 0;
        };
      };
    }

    # ── disko integration: service ordering ──────────────────────
    (mkIf cfg.diskoIntegration {
      systemd.services.nix-secret-bridge-installer = {
        before = [ "disko.service" ];
        requiredBy = [ "disko.service" ];
      };
    })

    # ── Cleanup service ──────────────────────────────────────────
    (mkIf cfg.cleanupAfterDisko {
      systemd.services.nix-secret-bridge-cleanup = {
        description = "nix-secret-bridge: Securely clean up decrypted bootstrap secrets";

        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${bridgePkg}/bin/nix-secret-bridge cleanup --mount-base ${cfg.mountBase}";

          StandardOutput = "null";
          StandardError = "journal";
        };

        after = lib.mkIf cfg.diskoIntegration [ "disko.service" ];
        wantedBy = lib.mkIf cfg.diskoIntegration [ "disko.service" ];

        unitConfig = {
          # Run cleanup even if disko fails
          DefaultDependencies = false;
        };
      };
    })
  ]);
}
