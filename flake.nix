{
  description = "Statup Rust application and NixOS service";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin" # Apple silicon
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

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

          # Generate the stylesheet after Cargo builds the application.
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

            cp -r static "$out/share/statup/"
            cp -r templates "$out/share/statup/"
            cp -r locales "$out/share/statup/"
            cp -r migrations "$out/share/statup/"

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
        in
        {
          options.services.statup = {
            enable = lib.mkEnableOption "Statup";

            package = lib.mkOption {
              type = lib.types.package;
              default = statupPackage;
              description = "Statup application package.";
            };

            port = lib.mkOption {
              type = lib.types.port;
              default = 3000;
              description = "HTTP port for Statup.";
            };

            host = lib.mkOption {
              type = lib.types.str;
              default = "0.0.0.0";
              description = "Address on which Statup listens.";
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
              home = "/var/lib/statup";
              createHome = true;
            };

            systemd.services.statup = {
              description = "Statup web application";
              wantedBy = [ "multi-user.target" ];
              after = [ "network.target" ];

              environment = {
                DATABASE_URL = "/var/lib/statup/statup.db";
                UPLOAD_DIR = "/var/lib/statup/uploads";
                HOST = cfg.host;
                PORT = toString cfg.port;
              };

              serviceConfig = {
                Type = "simple";
                User = "statup";
                Group = "statup";
                StateDirectory = "statup";
                WorkingDirectory = "${cfg.package}/share/statup";
                ExecStart = "${cfg.package}/libexec/statup";
                Restart = "on-failure";
                RestartSec = "5s";

                NoNewPrivileges = true;
                PrivateTmp = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                ReadWritePaths = [ "/var/lib/statup" ];
              };
            };

            networking.firewall.allowedTCPPorts = [ cfg.port ];
          };
        };
    };
}
