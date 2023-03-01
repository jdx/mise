{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    {
      overlays.rtx = final: prev: {
        rtx = prev.callPackage ./default.nix { };
      };
    } // flake-utils.lib.eachDefaultSystem(system:
      let
        pkgs = import nixpkgs { inherit system; };
        rtx = pkgs.callPackage ./default.nix { };
      in
        {
          packages = {
            inherit rtx;
            default = rtx;
          };

          devShells.default = pkgs.mkShell {
            name = "rtx-develop";

            inputsFrom = [ rtx ];

            nativeBuildInputs = with pkgs; [
              just
              clippy
              rustfmt
              shellcheck
              shfmt
              nodejs
              cargo-release
            ];
          };
        }
    );
}
