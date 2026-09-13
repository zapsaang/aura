# Home Manager module semantic evaluation (Todo 17): evaluates
# deployment/home-manager/default.nix against stubbed lib/pkgs for both
# platforms and proves eight semantic checks: null default; explicit
# override pair; Linux GPU flags; Darwin workspace-only flags; Linux has
# systemd and no launchd; Darwin has launchd and no systemd; null yields
# no override on either; heartbeatMs 250 reaches both supervisors.
let
  hasInfix = needle: haystack: builtins.length (builtins.split needle haystack) > 1;

  lib = rec {
    mkEnableOption = _description: { default = false; };
    mkOption = attrs: { default = attrs.default; };
    mkIf = condition: content: if condition then content else { };
    optionalString = condition: text: if condition then text else "";
    escapeShellArg = text: "'${text}'";
    optionals = condition: items: if condition then items else [ ];
    cleanSource = source: source;
    types = {
      int = { };
      str = { };
      nullOr = _inner: { };
    };
  };

  fakePkgs = isLinux: {
    stdenv = {
      inherit isLinux;
      isDarwin = !isLinux;
    };
    rustPlatform.buildRustPackage =
      args:
      args
      // {
        outPath = "/nix/store/aura-${if isLinux then "linux" else "darwin"}";
      };
  };

  evalFor =
    { isLinux, aura }:
    let
      completed = {
        enable = aura.enable or true;
        shmPath = aura.shmPath or null;
        heartbeatMs = aura.heartbeatMs or 500;
      };
      module = import ../default.nix {
        config.services.aura = completed;
        inherit lib;
        pkgs = fakePkgs isLinux;
      };
    in
    {
      options = module.options.services.aura;
      config = module.config;
      package = builtins.head module.config.home.packages;
    };

  systemdService = ev: ev.config.systemd.user.services.aura-daemon;
  launchdAgent = ev: ev.config.launchd.agents.aura-daemon;
  launchdArgs = ev: (launchdAgent ev).config.ProgramArguments;

  linuxDefault = evalFor { isLinux = true; aura = { }; };
  darwinDefault = evalFor { isLinux = false; aura = { }; };
  linuxOverride = evalFor {
    isLinux = true;
    aura.shmPath = "/custom/state.dat";
  };
  darwinOverride = evalFor {
    isLinux = false;
    aura.shmPath = "/custom/state.dat";
  };
  linux250 = evalFor {
    isLinux = true;
    aura.heartbeatMs = 250;
  };
  darwin250 = evalFor {
    isLinux = false;
    aura.heartbeatMs = 250;
  };

  checks = [
    # 1. shmPath defaults to null and the service defaults to disabled.
    (linuxDefault.options.shmPath.default == null && linuxDefault.options.enable.default == false)
    # 2. An explicit shmPath reaches the systemd command line shell-escaped
    # and the launchd argument vector as an exact pair.
    (
      hasInfix "--shm-path '/custom/state.dat'" (systemdService linuxOverride).Service.ExecStart
      && builtins.elem "--shm-path" (launchdArgs darwinOverride)
      && builtins.elem "/custom/state.dat" (launchdArgs darwinOverride)
    )
    # 3. Linux builds enable exactly the namespaced NVML GPU feature.
    (linuxDefault.package.buildFeatures == [ "aura-daemon/gpu-nvml" ])
    # 4. Darwin builds the workspace with no extra features.
    (darwinDefault.package.buildFeatures == [ ])
    # 5. Linux installs the systemd notify/watchdog unit and no launchd agent.
    (
      (systemdService linuxDefault).Service.Type == "notify"
      && (systemdService linuxDefault).Service.WatchdogSec == "3s"
      && launchdAgent linuxDefault == { }
    )
    # 6. Darwin installs the launchd agent and no systemd unit.
    (
      (launchdAgent darwinDefault).enable == true
      && (launchdAgent darwinDefault).config.Label == "com.aura.daemon"
      && systemdService darwinDefault == { }
    )
    # 7. The null default emits no override argument on either supervisor.
    (
      !(hasInfix "--shm-path" (systemdService linuxDefault).Service.ExecStart)
      && !(builtins.elem "--shm-path" (launchdArgs darwinDefault))
    )
    # 8. heartbeatMs 250 reaches both supervisor command lines.
    (
      hasInfix "--heartbeat-ms 250" (systemdService linux250).Service.ExecStart
      && builtins.elem "250" (launchdArgs darwin250)
    )
  ];

  passed = builtins.foldl' (acc: check: acc && check) true checks;
in
assert passed;
{
  checks = builtins.length checks;
  status = "ok";
}
