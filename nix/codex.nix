{
  lib,
  stdenvNoCC,
  fetchurl,
}:
let
  releases = {
    x86_64-linux = {
      target = "x86_64-unknown-linux-musl";
      sha256 = "04d8ab9dbcb9df0edf3c67dca5072a374babfdf762a9bc4ae649ae140b8e2cf0";
    };
    aarch64-linux = {
      target = "aarch64-unknown-linux-musl";
      sha256 = "3c02e2ae34be0d06e62557e98fc5c0a783bec5a2fed406fe00e565803bf84ee8";
    };
    x86_64-darwin = {
      target = "x86_64-apple-darwin";
      sha256 = "53f7c9081ab83684a3d4fb35dbc4b05d3ebb204beb2eab000626c15f0b15e738";
    };
    aarch64-darwin = {
      target = "aarch64-apple-darwin";
      sha256 = "f0feee8537daf8dd6b4e0a36e764549ad14afdbb419d8775aeab9feb20182313";
    };
  };
  release = releases.${stdenvNoCC.hostPlatform.system};
in
stdenvNoCC.mkDerivation rec {
  pname = "relay-codex";
  version = "0.161.0";
  src = fetchurl {
    url = "https://github.com/openai/codex/releases/download/rust-v${version}/codex-package-${release.target}.tar.gz";
    inherit (release) sha256;
  };
  sourceRoot = ".";
  dontBuild = true;
  dontStrip = true;
  dontPatchELF = true;
  dontPatchShebangs = true;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out"
    cp -R bin codex-path codex-resources codex-package.json "$out/"
    runHook postInstall
  '';
  meta = {
    description = "Pinned official Codex CLI and bundled sandbox resources for Relay";
    homepage = "https://github.com/openai/codex";
    license = lib.licenses.asl20;
    platforms = builtins.attrNames releases;
    mainProgram = "codex";
  };
}
