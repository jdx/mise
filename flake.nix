{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem(system:
      let
        pkgs = import nixpkgs { inherit system; };
        rtx = pkgs.callPackage ./default.nix { };
      in
        {
          packages = {
            inherit rtx;
            default = rtx;
          };
        }
    );
}
