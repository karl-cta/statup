{
  description = "Statup Rust application and NixOS service";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      linuxSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      systems = linuxSystems ++ [
        "aarch64-darwin"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;
      forLinuxSystems = nixpkgs.lib.genAttrs linuxSystems;

      packageFor = system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          manifest = (pkgs.lib.importTOML ./Cargo.toml).package;

        in
        pkgs.rustPlatform.buildRustPackage {
          pname = manifest.name;
          version = manifest.version;

          src = nixpkgs.lib.cleanSource ./.;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [
            pkgs.tailwindcss_4
          ];

          postBuild = ''
            tailwindcss \
              --input static/css/input.css \
              --output static/css/style.css \
              --minify

            find static/css \
              -name '*.css' \
              ! -name style.css \
              ! -name setup-transitions.css \
              -delete

            find static/css \
              -mindepth 1 \
              -type d \
              -exec rm -rf {} +
          '';

          postInstall = ''
            mkdir -p "$out/share/statup"

            # Templates, translations and migrations are compiled into the
            # binary: only the static files are read from disk.
            cp -r static "$out/share/statup/"

            install -Dm644 LICENSE \
              "$out/share/licenses/statup/LICENSE"

            install -Dm644 THIRD_PARTY_NOTICES.md \
              "$out/share/licenses/statup/THIRD_PARTY_NOTICES.md"

            # The application expects its assets relative to its working
            # directory, as it does in the Docker image.
            mkdir -p "$out/libexec"
            mv "$out/bin/statup" "$out/libexec/statup"

            mkdir -p "$out/bin"
            cat > "$out/bin/statup" <<EOF
            #!${pkgs.runtimeShell}
            cd "$out/share/statup"
            exec "$out/libexec/statup" "\$@"
            EOF
            chmod +x "$out/bin/statup"
          '';

          doCheck = false;

          meta = {
            description = "Statup web application";
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (system: {
        default = packageFor system;
        statup = packageFor system;
      });

      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.statup;
          statupPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          stateDir = "/var/lib/statup";
          toEnv = value: if builtins.isBool value then lib.boolToString value else toString value;
        in
        {
          options.services.statup = {
            enable = lib.mkEnableOption "Statup";

            package = lib.mkOption {
              type = lib.types.package;
              default = statupPackage;
              defaultText = lib.literalExpression "statup.packages.\${system}.default";
              description = "Statup application package.";
            };

            settings = lib.mkOption {
              type = lib.types.submodule {
                freeformType = with lib.types; attrsOf (oneOf [ str int bool ]);

                options = {
                  HOST = lib.mkOption {
                    type = lib.types.str;
                    default = "0.0.0.0";
                    description = "Address on which Statup listens.";
                  };

                  PORT = lib.mkOption {
                    type = lib.types.port;
                    default = 3000;
                    description = "HTTP port for Statup.";
                  };
                };
              };
              default = { };
              example = {
                PUBLIC_URL = "https://status.example.com";
                TZ = "Europe/Dublin";
              };
              description = ''
                Settings of Statup, passed as environment variables, as listed
                at https://statup.dev/docs/configuration/. The database and the
                uploads stay in ${stateDir}. Secrets go in environmentFile, out
                of the Nix store.
              '';
            };

            environmentFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              example = "/run/secrets/statup.env";
              description = ''
                A file of KEY=value lines read by systemd when the service
                starts, for secrets such as SMTP_PASSWORD.
              '';
            };

            openFirewall = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Open the port of Statup in the firewall.";
            };
          };

          config = lib.mkIf cfg.enable {
            users.groups.statup = {
              gid = 10001;
            };

            users.users.statup = {
              isSystemUser = true;
              uid = 10001;
              group = "statup";
              home = stateDir;
              createHome = true;
            };

            # For the maintenance commands, such as reset-password.
            environment.systemPackages = [ cfg.package ];

            systemd.services.statup = {
              description = "Statup web application";
              wantedBy = [ "multi-user.target" ];
              after = [ "network.target" ];

              # The only paths the hardening below leaves writable.
              environment = lib.mapAttrs (_: toEnv) cfg.settings // {
                DATABASE_URL = "${stateDir}/statup.db";
                UPLOAD_DIR = "${stateDir}/uploads";
              };

              serviceConfig = {
                Type = "simple";
                User = "statup";
                Group = "statup";
                StateDirectory = "statup";
                WorkingDirectory = "${cfg.package}/share/statup";
                ExecStart = "${cfg.package}/libexec/statup";
                EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
                Restart = "on-failure";
                RestartSec = "5s";

                NoNewPrivileges = true;
                PrivateTmp = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                ReadWritePaths = [ stateDir ];
              };
            };

            networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.settings.PORT ];
          };
        };

      # Boots a NixOS machine with the service, as the documentation sets it
      # up, and runs the commands the documentation gives.
      checks = forLinuxSystems (system: {
        nixos = nixpkgs.legacyPackages.${system}.testers.runNixOSTest {
          name = "statup";

          nodes.machine = { pkgs, ... }: {
            imports = [ self.nixosModules.default ];

            services.statup = {
              enable = true;
              settings.TZ = "Europe/Dublin";
              environmentFile = "/etc/statup.env";
            };

            # A throwaway administrator for the check, created at first start.
            environment.etc."statup.env".text = ''
              ADMIN_EMAIL=admin@example.com
              ADMIN_PASSWORD=nixos-check-password-0001
            '';

            environment.systemPackages = [ pkgs.curl ];
          };

          testScript = ''
            machine.wait_for_unit("statup.service")
            machine.wait_for_open_port(3000)
            machine.succeed("curl --fail http://127.0.0.1:3000/health")
            machine.succeed("curl --fail http://127.0.0.1:3000/login")
            machine.succeed("curl --fail http://127.0.0.1:3000/static/css/style.css")
            machine.succeed(
                "sudo -u statup env DATABASE_URL=/var/lib/statup/statup.db"
                " statup reset-password admin@example.com"
            )
          '';
        };
      });
    };
}
