self:
{
  lib,
  pkgs,
  config,
  inputs,
  ...
}:
let
  cfg = config.services.ahgroupbot;
  defaultPkg = self.packages.${pkgs.system}.default;
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

        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
      };
      environment = {
        RUST_LOG = "ahgroupbot=${cfg.logLevel},warn";
      };
    };
  };
}
