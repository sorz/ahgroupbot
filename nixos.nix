self:
{
  lib,
  pkgs,
  config,
  ...
}:
let
  cfg = config.services.ahgroupbot;
  defaultPkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  options.services.ahgroupbot = {
    enable = lib.mkEnableOption "AhAhAh Group Telegram Bot";
    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPkg;
      description = "Package to run for the ahgroupbot service.";
    };
    chatId = lib.mkOption {
      type = lib.types.int;
      description = "Group's chat ID the bot manage for";
    };
    apiTokenFile = lib.mkOption {
      type = lib.types.path;
      description = "Text file that contains Telegram API token";
    };
    logLevel = lib.mkOption {
      type = lib.types.enum [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
        "off"
      ];
      default = "info";
      description = "Log level";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.ahgroupbot = {
      description = "AhAhAh Group Telegram Bot";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/ahgroupbot ${toString cfg.chatId}";
        DynamicUser = true;
        StateDirectory = "ahgroupbot";
        Restart = "on-failure";
        RestartSec = 30;
        LoadCredential = [ "token:${cfg.apiTokenFile}" ];

        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectHostname = true;
        ProtectClock = true;
        ProtectProc = "invisible";
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        RestrictSUIDSGID = true;
        RestrictAddressFamilies = "AF_INET AF_INET6";
        LockPersonality = true;
        NoNewPrivileges = true;
        MemoryDenyWriteExecute = true;
        CapabilityBoundingSet = "";
        SystemCallArchitectures = "native";
        SystemCallFilter = "~@obsolete @clock @cpu-emulation @debug @keyring @module @mount @raw-io @swap";
      };
      environment = {
        RUST_LOG = "ahgroupbot=${cfg.logLevel},warn";
      };
    };
  };
}
