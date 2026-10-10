# NixOS module: `services.kaze`.
#
#   imports = [ kaze.nixosModules.default ];
#   services.kaze = {
#     enable = true;
#     libraryDir = "/srv/manga";
#     openFirewall = true;
#     settings.settings.flaresolverr_url = "http://127.0.0.1:8191";
#   };
#
# `settings` is written to kaze.toml. Keys under `settings.settings` are
# locked in the UI, so this file stays the source of truth.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.kaze;
  toml = pkgs.formats.toml { };
  configFile = toml.generate "kaze.toml" (
    lib.recursiveUpdate {
      listen = "${cfg.address}:${toString cfg.port}";
      data_dir = cfg.dataDir;
      library_dir = cfg.libraryDir;
    } cfg.settings
  );
in
{
  options.services.kaze = {
    enable = lib.mkEnableOption "kaze, a self-hosted manga downloader";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "kaze.packages.\${system}.default";
      description = "The kaze package to run.";
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0";
      description = "Address the web UI and API listen on.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 6531;
      description = "Port the web UI and API listen on.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/kaze";
      description = "Database and other state.";
    };

    libraryDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/kaze/library";
      description = "Where series folders and .cbz files go. Point Komga or Kavita here.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "kaze";
      description = "User kaze runs as. Created when left as `kaze`.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "kaze";
      description = "Group kaze runs as; give your library server read access through it.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open `port` in the firewall.";
    };

    settings = lib.mkOption {
      type = toml.type;
      default = { };
      example = lib.literalExpression ''
        {
          http.timeout_secs = 60;
          settings = {
            concurrency = 2;
            flaresolverr_url = "http://127.0.0.1:8191";
          };
        }
      '';
      description = ''
        Extra kaze.toml contents (see kaze.example.toml). Values under
        `settings` are locked in the UI.
      '';
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = "/run/secrets/kaze.env";
      description = ''
        File with `KAZE_*` variables kept out of the Nix store, such as
        `KAZE_SETTINGS__API_KEY=...`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    users.users = lib.mkIf (cfg.user == "kaze") {
      kaze = {
        isSystemUser = true;
        group = cfg.group;
        home = cfg.dataDir;
      };
    };
    users.groups = lib.mkIf (cfg.group == "kaze") { kaze = { }; };

    systemd.tmpfiles.settings."10-kaze" = {
      ${cfg.dataDir}.d = {
        inherit (cfg) user group;
        mode = "0750";
      };
      ${cfg.libraryDir}.d = {
        inherit (cfg) user group;
        mode = "0775";
      };
    };

    systemd.services.kaze = {
      description = "kaze manga downloader";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      environment.RUST_LOG = lib.mkDefault "kaze=info";
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve --config ${configFile}";
        User = cfg.user;
        Group = cfg.group;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "on-failure";
        # Group-readable files, so a library server in the group can read them.
        UMask = "0002";
        ReadWritePaths = [
          cfg.dataDir
          cfg.libraryDir
        ];
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        SystemCallArchitectures = "native";
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
  };
}
