{
  description = "Relay — native, issue-centered agent workspace";
  inputs = {
    mosaic.url = "git+ssh://git@gitlab.com/unincorporated/mosaic/mosaic.git?ref=main&rev=98e1f838a0b9b925a0d3cfa737d3eb20d299b1b8";
    # Share dependency pins with Mosaic while defining Relay's own environment.
    nixpkgs.follows = "mosaic/nixpkgs";
    rust-overlay.follows = "mosaic/rust-overlay";
    flake-utils.follows = "mosaic/flake-utils";
  };

  outputs =
    {
      self,
      mosaic,
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
          config.allowUnfreePredicate = pkg: builtins.elem (pkgs.lib.getName pkg) [ "claude-code" ];
        };
        rust = pkgs.rust-bin.stable."1.89.0".default.override {
          targets = [ "wasm32-unknown-unknown" ];
          extensions = [
            "rust-analyzer"
            "rust-src"
            "rustfmt"
            "clippy"
          ];
        };
        relayCodex = pkgs.callPackage ./nix/codex.nix { };
        tools =
          with pkgs;
          [
            rust
            bash
            just
            pkg-config
            git
            gh
            glab
            relayCodex
            claude-code
            curl
            jq
            openssl
            sqlite
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.bubblewrap
            pkgs.socat
          ];
        desktopLibraries = pkgs.lib.optionals pkgs.stdenv.isLinux (
          with pkgs;
          [
            vulkan-loader
            libGL
            wayland
            libxkbcommon
            libx11
            libxcursor
            libxi
            libxrandr
          ]
        );
        mkRelayShell =
          libraries: extraTools:
          pkgs.mkShell (
            {
              name = "relay";
              packages = tools ++ extraTools;
              buildInputs = libraries;
              RUST_BACKTRACE = "1";
            }
            // pkgs.lib.optionalAttrs (pkgs.stdenv.isLinux && libraries != [ ]) {
              # wgpu and winit load their native libraries dynamically.
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries;
            }
          );
      in
      {
        devShells = {
          default = mkRelayShell desktopLibraries [ ];
          server = mkRelayShell [ ] [ ];
          web =
            mkRelayShell
              [ ]
              [
                pkgs.wasm-bindgen-cli
                mosaic.packages.${system}.mosaic-cli
                pkgs.python3
              ];
          browser = mkRelayShell desktopLibraries [
            pkgs.chromium
            pkgs.nodejs
            pkgs.python3
          ];
          ui-test = mkRelayShell desktopLibraries (
            pkgs.lib.optionals pkgs.stdenv.isLinux (
              with pkgs;
              (map lib.getBin [
                xorg-server
                xdotool
                imagemagick
                xclip
              ])
              ++ [ mesa ]
            )
          );
        };
        packages = {
          inherit (mosaic.packages.${system}) mosaic-cli mosaic-fmt mosaic-lsp;
          relay-server = import ./nix/server-package.nix {
            inherit pkgs;
            rust = pkgs.rust-bin.stable."1.89.0".default;
            src = self;
          };
          relay-web = import ./nix/web-package.nix {
            inherit pkgs rust;
            src = self;
            mosaicSource = mosaic;
            mosaicCli = mosaic.packages.${system}.mosaic-cli;
          };
          default = self.packages.${system}.relay-server;
        };
        apps.relay-server = {
          type = "app";
          program = "${self.packages.${system}.relay-server}/bin/relay-server";
        };
        formatter = pkgs.nixfmt-tree;
      }
    )
    // {
      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };
      nixosModules.default = import ./nix/nixos.nix { inherit self; };
    };
}
