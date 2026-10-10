{
  description = "Relay server packages and services without private Mosaic dependencies";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/b5aa0fbd538984f6e3d201be0005b4463d8b09f8";
    rust-overlay = {
      url = "github:oxalica/rust-overlay/e3fa5cf86b93914b8f312b2a1ca14fbb139c655c";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rust = pkgs.rust-bin.stable."1.89.0".default;
          relay-server = import ../server-package.nix {
            inherit pkgs rust;
            src = ../..;
          };
        in
        {
          inherit relay-server;
          default = relay-server;
        }
      );
      apps = forSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.relay-server}/bin/relay-server";
        };
        relay-server = self.apps.${system}.default;
      });
      homeManagerModules.default = import ../home-manager.nix { inherit self; };
      nixosModules.default = import ../nixos.nix { inherit self; };
      formatter = forSystems (system: nixpkgs.legacyPackages.${system}.nixfmt-tree);
    };
}
