{
  lib,
  pkgs,
  relayPackage,
  configDirectory,
  dataDirectory,
}:
with lib;
{
  enable = mkEnableOption "the persistent Relay workspace server";
  package = mkOption {
    type = types.package;
    default = relayPackage;
    description = "Relay server executable package.";
  };
  webPackage = mkOption {
    type = types.nullOr types.package;
    default = null;
    description = "Mosaic WASM application to serve at /app/.";
  };
  tokenFile = mkOption {
    type = types.str;
    default = "${configDirectory}/admin-token";
    description = "Absolute runtime file containing the native administrator token, outside the Nix store.";
  };
  setupTokenFile = mkOption {
    type = types.nullOr types.str;
    default = "${configDirectory}/setup-token";
    description = "Runtime owner-enrollment code file; null disables enrollment.";
  };
  generateTokens = mkOption {
    type = types.bool;
    default = true;
    description = "Generate missing private token files once; retain them on restart.";
  };
  databasePath = mkOption {
    type = types.str;
    default = "${dataDirectory}/workspace.sqlite3";
    description = "Persistent SQLite workspace; only one server may own this database.";
  };
  listenAddress = mkOption {
    type = types.str;
    default = "127.0.0.1";
    description = "HTTP listen address; keep loopback when using a TLS proxy.";
  };
  port = mkOption {
    type = types.port;
    default = 7331;
    description = "HTTP port.";
  };
  publicOrigin = mkOption {
    type = types.nullOr types.str;
    default = null;
    description = "Exact HTTPS origin enabling browser authentication, including a non-default port.";
  };
  extraPackages = mkOption {
    type = types.listOf types.package;
    default = [ ];
    description = "Additional tools available to server-owned harness processes.";
  };
  environment = mkOption {
    type = types.attrsOf types.str;
    default = { };
    description = "Additional non-secret runtime settings; secret values must stay outside Nix expressions.";
  };
  funnel = {
    enable = mkEnableOption "public HTTPS through an already enrolled Tailscale node";
    package = mkOption {
      type = types.package;
      default = pkgs.tailscale;
      description = "Tailscale client executable.";
    };
    socket = mkOption {
      type = types.str;
      default = "/run/tailscale/tailscaled.sock";
      description = "Socket of the enrolled Tailscale daemon.";
    };
    httpsPort = mkOption {
      type = types.enum [
        443
        8443
        10000
      ];
      default = 8443;
      description = "Public Funnel port; choose a port not serving another application.";
    };
    daemonService = mkOption {
      type = types.str;
      default = "tailscaled.service";
      description = "Tailscale daemon unit in the same systemd manager.";
    };
  };
}
