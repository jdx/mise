{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      overlay = final: prev: {
        mise = prev.callPackage ./default.nix { };
      };
    } // flake-utils.lib.eachDefaultSystem(system:
      let
        pkgs = import nixpkgs { inherit system; };
        mise = pkgs.callPackage ./default.nix { };
      in
        {
          packages = {
            inherit mise;
            default = mise;
          };

          devShells.default = pkgs.mkShell {
            name = "mise-develop";

            inputsFrom = [ mise ];

            nativeBuildInputs = with pkgs; [
              just
              clippy
              rustfmt
              shellcheck
              shfmt
              nodejs
              cargo-release
              cargo-insta
            ];
          };
        }
    );
}
