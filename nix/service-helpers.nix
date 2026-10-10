{ pkgs, cfg }:
let
  tokenPaths = [ cfg.tokenFile ] ++ pkgs.lib.optional (cfg.setupTokenFile != null) cfg.setupTokenFile;
in
{
  initialize = pkgs.writeShellApplication {
    name = "relay-initialize";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.openssl
    ];
    text = ''
      umask 077
      ${pkgs.lib.concatMapStringsSep "\n" (path: ''
        relay_token_file=${pkgs.lib.escapeShellArg path}
        if [[ ! -e "$relay_token_file" && ! -L "$relay_token_file" ]]; then
          mkdir -p "$(dirname "$relay_token_file")"
          (set -o noclobber; openssl rand -hex 32 > "$relay_token_file")
        fi
      '') tokenPaths}
    '';
  };
  start = pkgs.writeShellApplication {
    name = "relay-start";
    text = ''
      unset RELAY_TOKEN
      export RELAY_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-token"
      ${pkgs.lib.optionalString (cfg.setupTokenFile != null) ''
        export RELAY_SETUP_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-setup"
      ''}
      exec ${cfg.package}/bin/relay-server
    '';
  };
  credentials = [
    "relay-token:${cfg.tokenFile}"
  ]
  ++ pkgs.lib.optional (cfg.setupTokenFile != null) "relay-setup:${cfg.setupTokenFile}";
  environment = {
    RELAY_BIND = "${cfg.listenAddress}:${toString cfg.port}";
    RELAY_DATABASE = cfg.databasePath;
    SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    NODE_EXTRA_CA_CERTS = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  }
  // pkgs.lib.optionalAttrs (cfg.publicOrigin != null) { RELAY_PUBLIC_ORIGIN = cfg.publicOrigin; }
  // pkgs.lib.optionalAttrs (cfg.webPackage != null) { RELAY_WEB_ROOT = toString cfg.webPackage; }
  // cfg.environment;
  runtimePackages = [
    pkgs.git
    pkgs.openssh
    pkgs.gh
    pkgs.glab
    pkgs.bash
    pkgs.coreutils
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
    pkgs.bubblewrap
    pkgs.socat
  ]
  ++ cfg.extraPackages;
  assertions = map (path: {
    assertion = pkgs.lib.hasPrefix "/" path && !(pkgs.lib.hasPrefix "/nix/store/" path);
    message = "Relay secret files must be absolute runtime paths outside the Nix store.";
  }) tokenPaths;
  funnelCommand = "${cfg.funnel.package}/bin/tailscale --socket=${cfg.funnel.socket} funnel --yes --https=${toString cfg.funnel.httpsPort} http://127.0.0.1:${toString cfg.port}";
}
