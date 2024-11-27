{
  description = "Pyro Discord Bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    advisory-db,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = nixpkgs.legacyPackages.${system};
      inherit (pkgs) lib;

      craneLib = crane.mkLib pkgs;
      src = craneLib.cleanCargoSource ./.;

      commonArgs = {
        inherit src;
        strictDeps = true;

        buildInputs =
          [
            pkgs.openssl
            pkgs.pkg-config
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

        DISCORD_TOKEN = ""; # Token will be provided via environment
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      pyrobot = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
        });

      # NixOS module for the Pyrobot service
      pyrobotModule = {
        config,
        lib,
        pkgs,
        ...
      }: {
        options.services.pyrobot = {
          enable = lib.mkEnableOption "Pyro Discord Bot";
          tokenFile = lib.mkOption {
            type = lib.types.path;
            description = "File containing the Discord bot token";
          };
          user = lib.mkOption {
            type = lib.types.str;
            default = "pyrobot";
            description = "User account under which the bot runs";
          };
          group = lib.mkOption {
            type = lib.types.str;
            default = "pyrobot";
            description = "Group under which the bot runs";
          };
        };

        config = lib.mkIf config.services.pyrobot.enable {
          users.users.${config.services.pyrobot.user} = {
            isSystemUser = true;
            group = config.services.pyrobot.group;
            description = "Pyrobot Discord bot service user";
          };

          users.groups.${config.services.pyrobot.group} = {};

          systemd.services.pyrobot = {
            description = "Pyrobot Discord Bot";
            wantedBy = ["multi-user.target"];
            after = ["network-online.target"];
            wants = ["network-online.target"];

            serviceConfig = {
              Type = "simple";
              User = config.services.pyrobot.user;
              Group = config.services.pyrobot.group;
              ExecStart = "${pyrobot}/bin/pyrobot";
              Restart = "always";
              RestartSec = "30s";

              # Security hardening
              NoNewPrivileges = true;
              PrivateTmp = true;
              PrivateDevices = true;
              ProtectSystem = "strict";
              ProtectHome = true;
              ReadOnlyDirectories = "/";
              ReadWritePaths = [];
              PrivateUsers = true;

              # Environment setup
              EnvironmentFile = config.services.pyrobot.tokenFile;
            };
          };
        };
      };
    in {
      checks = {
        inherit pyrobot;

        pyrobot-clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

        pyrobot-fmt = craneLib.cargoFmt {
          inherit src;
        };

        pyrobot-audit = craneLib.cargoAudit {
          inherit src advisory-db;
        };

        pyrobot-nextest = craneLib.cargoNextest (commonArgs
          // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
      };

      packages = {
        default = pyrobot;
      };

      nixosModules.default = pyrobotModule;

      apps.default = flake-utils.lib.mkApp {
        drv = pyrobot;
      };

      devShells.default = craneLib.devShell {
        checks = self.checks.${system};

        packages = with pkgs; [
          pkg-config
          openssl
          rust-analyzer
          alejandra
        ];
      };
    });
}
