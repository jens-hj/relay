{
  description = "Relay — native, issue-centered agent workspace";
  inputs = {
    mosaic.url = "git+https://gitlab.com/unincorporated/mosaic/mosaic.git?rev=658ccfa168fcdccea790b81d591ff263e555ce61";
    # Share dependency pins with Mosaic while defining Relay's own environment.
    nixpkgs.follows = "mosaic/nixpkgs";
    rust-overlay.follows = "mosaic/rust-overlay";
    flake-utils.follows = "mosaic/flake-utils";
  };

  outputs =
    {
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
        };
        rust = pkgs.rust-bin.stable."1.89.0".default.override {
          extensions = [
            "rust-analyzer"
            "rust-src"
            "rustfmt"
            "clippy"
          ];
        };
        tools = with pkgs; [
          rust
          pkg-config
          git
          curl
          jq
          openssl
          sqlite
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
          libraries:
          pkgs.mkShell (
            {
              name = "relay";
              packages = tools;
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
          default = mkRelayShell desktopLibraries;
          server = mkRelayShell [ ];
        };
        packages = {
          inherit (mosaic.packages.${system}) mosaic-cli mosaic-fmt mosaic-lsp;
        };
        formatter = pkgs.nixfmt-tree;
      }
    );
}
