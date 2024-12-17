{
  description = "Pyro Discord Bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        inherit (pkgs) lib;

        craneLib = crane.mkLib pkgs;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            (lib.fileset.maybeMissing ./extra)
          ];
        };

        commonArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [
            pkgs.openssl
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];

          DISCORD_TOKEN = "";
        };

        pyrobot = craneLib.buildPackage (commonArgs // {
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        });

        pyrobotModule = { config, lib, pkgs, ... }: {
          options.services.pyrobot = {
            enable = lib.mkEnableOption "Pyro Discord Bot";
            token = lib.mkOption {
              type = lib.types.str;
              description = "Discord bot token";
            };
            workingDir = lib.mkOption {
              type = lib.types.str;
              default = "/var/lib/pyrobot";
              description = "Working directory for storage";
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
              description = "Pyro Discord bot service user";
              home = config.services.pyrobot.workingDir;
              createHome = true;
            };

            users.groups.${config.services.pyrobot.group} = {};

            systemd.services.pyrobot = {
              description = "Pyro Discord Bot";
              wantedBy = [ "multi-user.target" ];
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];

              serviceConfig = {
                Type = "simple";
                User = config.services.pyrobot.user;
                Group = config.services.pyrobot.group;
                ExecStart = "${pyrobot}/bin/pyrobot";
                Restart = "always";
                RestartSec = "30s";
                NoNewPrivileges = true;
                PrivateTmp = true;
                PrivateDevices = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                WorkingDirectory = config.services.pyrobot.workingDir;
                ReadOnlyDirectories = "/";
                ReadWritePaths = [ 
                  config.services.pyrobot.workingDir 
                ];
                PrivateUsers = true;
                Environment = "DISCORD_TOKEN=${config.services.pyrobot.token}";
              };
            };
          };
        };
      in
      {
        checks = {
          inherit pyrobot;
        };

        packages.default = pyrobot;

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
