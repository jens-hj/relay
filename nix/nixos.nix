{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.relay;
  helpers = import ./service-helpers.nix { inherit pkgs cfg; };
in
{
  options.services.relay =
    (import ./service-options.nix {
      inherit lib pkgs;
      relayPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.relay-server;
      configDirectory = "/var/lib/relay";
      dataDirectory = "/var/lib/relay";
    })
    // {
      user = lib.mkOption {
        type = lib.types.str;
        default = "relay";
        description = "Service user; use an existing user to reuse its harness/provider credentials.";
      };
      homeDirectory = lib.mkOption {
        type = lib.types.str;
        default = config.users.users.${cfg.user}.home;
        description = "Home used for installed harness, provider and SSH configuration.";
      };
    };
  config = lib.mkIf cfg.enable {
    assertions = helpers.assertions;
    users.groups.relay = lib.mkIf (cfg.user == "relay") { };
    users.users.relay = lib.mkIf (cfg.user == "relay") {
      isSystemUser = true;
      group = "relay";
      home = "/var/lib/relay";
    };
    systemd.services.relay-initialize = lib.mkIf cfg.generateTokens {
      description = "Initialize private Relay runtime credentials";
      serviceConfig = {
        Type = "oneshot";
        User = cfg.user;
        StateDirectory = "relay";
        StateDirectoryMode = "0700";
        ExecStart = "${helpers.initialize}/bin/relay-initialize";
        RemainAfterExit = true;
        UMask = "0077";
      };
    };
    systemd.services.relay = {
      description = "Relay workspace and agent server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ] ++ lib.optional cfg.generateTokens "relay-initialize.service";
      requires = lib.optional cfg.generateTokens "relay-initialize.service";
      path = helpers.runtimePackages;
      environment = helpers.environment // {
        HOME = cfg.homeDirectory;
      };
      serviceConfig = {
        User = cfg.user;
        StateDirectory = "relay";
        StateDirectoryMode = "0700";
        WorkingDirectory = cfg.homeDirectory;
        ExecStart = "${helpers.start}/bin/relay-start";
        LoadCredential = helpers.credentials;
        UMask = "0077";
        Restart = "on-failure";
        RestartSec = 5;
        KillMode = "control-group";
        TimeoutStopSec = 60;
      };
    };
    systemd.services.relay-funnel = lib.mkIf cfg.funnel.enable {
      description = "Public HTTPS access to authenticated Relay";
      wantedBy = [ "multi-user.target" ];
      after = [
        "relay.service"
        cfg.funnel.daemonService
      ];
      requires = [
        "relay.service"
        cfg.funnel.daemonService
      ];
      partOf = [
        "relay.service"
        cfg.funnel.daemonService
      ];
      serviceConfig = {
        ExecStart = helpers.funnelCommand;
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
