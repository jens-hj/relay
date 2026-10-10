{
  pkgs,
  rust,
  src,
}:
let
  root = /. + builtins.unsafeDiscardStringContext (toString src);
  filtered = pkgs.lib.fileset.toSource {
    inherit root;
    fileset = pkgs.lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/profiles")
      (root + "/crates/relay-core")
      (root + "/crates/relay-server")
    ];
  };
  serverSource = pkgs.runCommand "relay-server-source" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    python3 ${./prepare-server-source.py} ${filtered} "$out" --lock ${./server-Cargo.lock}
  '';
  platform = pkgs.makeRustPlatform {
    cargo = rust;
    rustc = rust;
  };
in
platform.buildRustPackage {
  pname = "relay-server";
  version = (builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"))).workspace.package.version;
  src = serverSource;
  cargoLock.lockFile = ./server-Cargo.lock;
  cargoBuildFlags = [
    "-p"
    "relay-server"
  ];
  # Domain/network/process checks run separately via just check. This package
  # can build in a sandbox without installed harnesses or a user's home.
  doCheck = false;
  meta = {
    description = "Persistent Relay workspace and agent server";
    homepage = "https://github.com/jens-hj/relay";
    license = with pkgs.lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "relay-server";
    platforms = pkgs.lib.platforms.unix;
  };
}
