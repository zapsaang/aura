{ config, lib, pkgs, ... }:

let
  cfg = config.services.aura;
  shmArgument = lib.optionalString (cfg.shmPath != null)
    " --shm-path ${lib.escapeShellArg cfg.shmPath}";
  auraPackage = pkgs.rustPlatform.buildRustPackage {
    pname = "aura";
    version = "0.1.0";
    src = lib.cleanSource ../../.;
    cargoLock.lockFile = ../../Cargo.lock;
  };
in
{
  options.services.aura = {
    enable = lib.mkEnableOption "AURA daemon user service";
    heartbeatMs = lib.mkOption {
      type = lib.types.int;
      default = 500;
      description = "AURA daemon heartbeat interval in milliseconds.";
    };
    shmPath = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional explicit shared memory path override for aura-daemon.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ auraPackage ];

    systemd.user.services.aura-daemon = lib.mkIf pkgs.stdenv.isLinux {
      Unit = {
        Description = "AURA daemon";
        After = [ "default.target" ];
      };
      Service = {
        Type = "notify";
        NotifyAccess = "main";
        WatchdogSec = "3s";
        RuntimeDirectory = "aura";
        RuntimeDirectoryMode = "0700";
        ExecStart = "${auraPackage}/bin/aura-daemon --heartbeat-ms ${toString cfg.heartbeatMs}${shmArgument}";
        Restart = "on-failure";
        Environment = [ "RUST_LOG=info" ];
        StandardOutput = "journal";
      };
      Install.WantedBy = [ "default.target" ];
    };

    launchd.agents.aura-daemon = lib.mkIf pkgs.stdenv.isDarwin {
      enable = true;
      config = {
        Label = "com.aura.daemon";
        ProgramArguments = [
          "${auraPackage}/bin/aura-daemon"
          "--heartbeat-ms"
          (toString cfg.heartbeatMs)
        ] ++ lib.optionals (cfg.shmPath != null) [ "--shm-path" cfg.shmPath ];
        RunAtLoad = true;
        KeepAlive = {};
        EnvironmentVariables = {
          RUST_LOG = "info";
        };
      };
    };
  };
}
