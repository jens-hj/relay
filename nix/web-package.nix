{
  pkgs,
  rust,
  src,
  mosaicCli,
  mosaicSource,
}:
let
  root = /. + builtins.unsafeDiscardStringContext (toString src);
  platform = pkgs.makeRustPlatform {
    cargo = rust;
    rustc = rust;
  };
  manifest = builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"));
  pinned = manifest.workspace.dependencies.mosaic;
  lock = builtins.fromTOML (builtins.readFile (root + "/Cargo.lock"));
  mosaicPackage = builtins.head (builtins.filter (package: package.name == "mosaic") lock.package);
  vendor = platform.importCargoLock.override {
    # Reuse the already fetched, published flake tree at the identical Cargo
    # revision. Cargo URLs and pins remain intact; no daemon SSH keys are needed.
    fetchgit =
      args:
      if args.url == pinned.git && args.rev == pinned.rev then
        assert mosaicSource.rev == pinned.rev;
        mosaicSource.outPath
      else
        pkgs.fetchgit args;
  };
in
platform.buildRustPackage {
  pname = "relay-web";
  version = (builtins.fromTOML (builtins.readFile (root + "/Cargo.toml"))).workspace.package.version;
  src = pkgs.lib.fileset.toSource {
    inherit root;
    fileset = pkgs.lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/profiles")
      (root + "/crates")
    ];
  };
  cargoDeps = vendor {
    lockFile = root + "/Cargo.lock";
    outputHashes."mosaic-${mosaicPackage.version}" = mosaicSource.narHash;
  };
  nativeBuildInputs = [
    mosaicCli
    pkgs.wasm-bindgen-cli
    pkgs.python3
  ];
  buildPhase = ''
    runHook preBuild
    cd crates/relay-desktop
    export CARGO_TARGET_DIR="$PWD/target"
    mosaic build web
    cd ../..
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    mkdir -p "$out"
    cp -r crates/relay-desktop/target/web/relay-desktop/. "$out/"
    python3 ${./prepare-web-shell.py} "$out"
    runHook postInstall
  '';
  doCheck = false;
  meta = {
    description = "Mosaic Relay application for WebAssembly and WebGPU";
    homepage = "https://github.com/jens-hj/relay";
    license = with pkgs.lib.licenses; [
      mit
      asl20
    ];
    platforms = pkgs.lib.platforms.unix;
  };
}
