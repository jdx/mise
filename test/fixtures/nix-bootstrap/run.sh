#!/usr/bin/env bash
set -euo pipefail

# Adapt only the test's copy of mise to the container's native dynamic loader.
mkdir -p /opt /work /test-home
cp /input/mise /opt/mise
chmod u+w /opt/mise
linker=$(nix eval --impure --raw --expr '(import <nixpkgs> {}).stdenv.cc.bintools.dynamicLinker')
patchelf --set-interpreter "$linker" --set-rpath /test-tools/lib /opt/mise
export PATH="/opt:$PATH"
export HOME=/test-home
export MISE_CONFIG_DIR=/test-home/.config/mise
export MISE_DATA_DIR=/test-home/.local/share/mise
export MISE_STATE_DIR=/test-home/.local/state/mise
export MISE_CACHE_DIR=/test-home/.cache/mise
export MISE_YES=1 MISE_TRUSTED_CONFIG_PATHS=/work
export MISE_USE_VERSIONS_HOST_TRACK=0
cd /work
mise --version
nix --version

# Config writing, status, and dry-run cannot initialize a profile or evaluate
# the requested source, which intentionally does not exist yet.
mise bootstrap packages use --no-install 'nix:git+file:///work/flake#hello'
mise bootstrap packages status --json | jq -e '.nix.packages[0].state == "missing"'
mise bootstrap packages apply --manager nix --dry-run
test ! -e "$HOME/.nix-profile"
test ! -L "$HOME/.nix-profile"
XDG_STATE_HOME=/work/xdg-state NIX_CONFIG="$NIX_CONFIG
use-xdg-base-directories = true" mise bootstrap packages status --json | jq -e '.nix.packages[0].state == "missing"'
test ! -e /work/xdg-state/nix/profile
test ! -L /work/xdg-state/nix/profile

mkdir flake
nixpkgs=$(nix eval --impure --raw --expr 'toString <nixpkgs>')
cat >flake/flake.nix <<NIX
{
  inputs.nixpkgs.url = "path:$nixpkgs";
  outputs = { self, nixpkgs }: let
    pkgs = import nixpkgs { system = "x86_64-linux"; };
    version = "v1";
  in {
    packages.x86_64-linux = {
      hello = pkgs.writeShellScriptBin "nix-bootstrap-hello" "echo \${version}";
      unrelated = pkgs.writeShellScriptBin "nix-bootstrap-unrelated" "echo \${version}";
      pinned = pkgs.writeShellScriptBin "nix-bootstrap-pinned" "echo \${version}";
      registryHello = pkgs.writeShellScriptBin "nix-bootstrap-registry" "echo \${version}";
    };
  };
}
NIX
nix flake lock path:/work/flake
git -C flake init -q
git -C flake config user.name 'Nix bootstrap test'
git -C flake config user.email 'test@example.invalid'
git -C flake add flake.nix flake.lock
git -C flake commit -qm 'test: first revision'
first=$(git -C flake rev-parse HEAD)
nix registry add nixpkgs git+file:///work/flake

# Start with an unrelated native profile package, then apply the declaration.
nix profile install 'git+file:///work/flake#unrelated'
mise bootstrap --only packages
test "$("$HOME/.nix-profile/bin/nix-bootstrap-hello")" = v1
test "$("$HOME/.nix-profile/bin/nix-bootstrap-unrelated")" = v1
before=$(readlink -f "$HOME/.nix-profile")
mise bootstrap packages apply --manager nix
test "$(readlink -f "$HOME/.nix-profile")" = "$before"
mise bootstrap packages status --json | jq -e '.nix.packages[0].state == "installed" and (.nix.packages[0].installed_version | startswith("/nix/store/"))'
mise bootstrap packages use nix:registryHello
test "$("$HOME/.nix-profile/bin/nix-bootstrap-registry")" = v1
before=$(readlink -f "$HOME/.nix-profile")
mise bootstrap packages apply --manager nix
test "$(readlink -f "$HOME/.nix-profile")" = "$before"

# Source identity matters even when another profile entry has the same attribute.
mise bootstrap packages apply "nix:git+file:///work/flake?rev=$first#pinned"
mise bootstrap packages use --no-install "nix:git+file:///work/flake?rev=$first#hello"
mise bootstrap packages status --json | jq -e '[.nix.packages[] | select(.state == "missing")] | length == 1'
mise bootstrap packages use --no-install "nix:git+file:///work/flake?rev=$first#pinned"
cat >mise.toml <<TOML
[bootstrap.packages]
"nix:git+file:///work/flake#hello" = "latest"
"nix:git+file:///work/flake?rev=$first#pinned" = "latest"
TOML

sed -i 's/version = "v1"/version = "v2"/' flake/flake.nix
git -C flake add flake.nix
git -C flake commit -qm 'test: second revision'
mise bootstrap packages upgrade --manager nix
test "$("$HOME/.nix-profile/bin/nix-bootstrap-hello")" = v2
test "$("$HOME/.nix-profile/bin/nix-bootstrap-unrelated")" = v1
test "$("$HOME/.nix-profile/bin/nix-bootstrap-pinned")" = v1

# Status remains usable with the package source absent and no network fetches.
mv flake hidden-flake
mise bootstrap packages status --json | jq -e 'all(.nix.packages[]; .state == "installed")'
mv hidden-flake flake

# A failed installation preserves the existing generation.
before=$(readlink -f "$HOME/.nix-profile")
if mise bootstrap packages apply 'nix:git+file:///work/flake#doesNotExist'; then
	echo 'expected missing attribute to fail' >&2
	exit 1
fi
test "$(readlink -f "$HOME/.nix-profile")" = "$before"

# Export is consumed by the real NixOS module evaluator, with the image's
# pinned package set. Build the selected packages, not a bootable system.
cat >mise.toml <<'TOML'
[bootstrap.packages]
"nix:hello" = "latest"
"nix:jq" = "latest"
TOML
mise bootstrap packages export --format nix >packages.nix
cat >system.nix <<NIX
let
  system = import ($nixpkgs + "/nixos") {
    system = "x86_64-linux";
    configuration = { imports = [ ./packages.nix ]; system.stateVersion = "26.05"; };
  };
  pkgs = system.pkgs;
  selected = builtins.filter (p: builtins.elem (p.pname or "") [ "hello" "jq" ]) system.config.environment.systemPackages;
in {
  names = map (p: p.pname) selected;
  bundle = pkgs.buildEnv { name = "mise-export-test"; paths = selected; };
}
NIX
nix-instantiate --eval --strict --json system.nix -A names | jq -e 'sort == ["hello", "jq"]'
bundle=$(nix-build --no-out-link system.nix -A bundle)
"$bundle/bin/hello"
"$bundle/bin/jq" --version
sed -i '/nix:hello/d' mise.toml
mise bootstrap packages export --format nix >packages.nix
nix-instantiate --eval --strict --json system.nix -A names | jq -e '. == ["jq"]'
printf '[bootstrap.packages]\n' >mise.toml
mise bootstrap packages export --format nix >packages.nix
nix-instantiate --eval --strict --json system.nix -A names | jq -e '. == []'
mise bootstrap packages use --no-install nix:thisAttributeDoesNotExist
mise bootstrap packages export --format nix >packages.nix
if nix-instantiate --eval --strict --json system.nix -A names; then
	echo 'expected missing exported attribute to fail' >&2
	exit 1
fi

echo 'Nix bootstrap Docker checks passed'
