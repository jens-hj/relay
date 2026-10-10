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
  options.services.relay = import ./service-options.nix {
    inherit lib pkgs;
    relayPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.relay-server;
    configDirectory = "${config.xdg.configHome}/relay";
    dataDirectory = "${config.xdg.dataHome}/relay";
  };
  config = lib.mkIf cfg.enable {
    assertions = helpers.assertions ++ [
      {
        assertion = pkgs.stdenv.isLinux;
        message = "Relay's Home Manager service requires Linux/systemd.";
      }
    ];
    home.packages = [ cfg.package ] ++ helpers.runtimePackages;
    systemd.user.services.relay-initialize = lib.mkIf cfg.generateTokens {
      Unit.Description = "Initialize private Relay runtime credentials";
      Service = {
        Type = "oneshot";
        ExecStart = "${helpers.initialize}/bin/relay-initialize";
        RemainAfterExit = true;
        UMask = "0077";
      };
    };
    systemd.user.services.relay = {
      Unit = {
        Description = "Relay workspace and agent server";
        After = [ "network.target" ] ++ lib.optional cfg.generateTokens "relay-initialize.service";
        Requires = lib.optional cfg.generateTokens "relay-initialize.service";
      };
      Service = {
        ExecStart = "${helpers.start}/bin/relay-start";
        WorkingDirectory = config.home.homeDirectory;
        LoadCredential = helpers.credentials;
        Environment = lib.mapAttrsToList (name: value: "${name}=${value}") (
          helpers.environment
          // {
            PATH = "${config.home.homeDirectory}/.local/bin:${config.home.homeDirectory}/.npm-global/bin:${config.home.homeDirectory}/.nix-profile/bin:/etc/profiles/per-user/${config.home.username}/bin:${lib.makeBinPath helpers.runtimePackages}:/run/current-system/sw/bin";
          }
        );
        UMask = "0077";
        Restart = "on-failure";
        RestartSec = 5;
        KillMode = "control-group";
        TimeoutStopSec = 60;
      };
      Install.WantedBy = [ "default.target" ];
    };
    systemd.user.services.relay-funnel = lib.mkIf cfg.funnel.enable {
      Unit = {
        Description = "Public HTTPS access to authenticated Relay";
        After = [
          "relay.service"
          cfg.funnel.daemonService
        ];
        Requires = [
          "relay.service"
          cfg.funnel.daemonService
        ];
        PartOf = [
          "relay.service"
          cfg.funnel.daemonService
        ];
      };
      Service = {
        ExecStart = helpers.funnelCommand;
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
