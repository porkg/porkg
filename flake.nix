{
  description = "Porkg package manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    gomod2nix = {
      url = "github:nix-community/gomod2nix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    gomod2nix,
  }: (
    flake-utils.lib.eachDefaultSystem
    (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [gomod2nix.overlays.default];
      };
    in {
      formatter = pkgs.alejandra;

      packages.porkg = pkgs.buildGoApplication {
        pname = "porkg";
        version = "0.1";
        src = ./.;
        pwd = ./.;
        modules = ./gomod2nix.toml;

        subPackages = ["cmd/porkg"];
      };

      devShells.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          go
          gopls
          gotools
          go-tools
          gomod2nix.packages.${system}.default
          sqlite-interactive
        ];
      };
    })
  );
}
