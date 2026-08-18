#!/usr/bin/env bash
# Idempotent Cloud Agent bootstrap. Safe to rerun; does not start long-lived processes.
set -euo pipefail

# Draft builds and some Cloud Agent images run as `ubuntu`, not root.
if [ "$(id -u)" -eq 0 ]; then
	SUDO=()
else
	SUDO=(sudo -n)
fi

export DEBIAN_FRONTEND=noninteractive
"${SUDO[@]}" apt-get update
"${SUDO[@]}" apt-get install -y --no-install-recommends libssl-dev pkg-config

msrv="$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
if [ -z "$msrv" ]; then
	echo "failed to parse rust-version from Cargo.toml" >&2
	exit 1
fi

rustup toolchain install "$msrv" --profile minimal --no-self-update -c rustfmt,clippy
rustup default "$msrv"

cargo build --all-features
"${SUDO[@]}" ln -sfn "$PWD/target/debug/mise" /usr/local/bin/mise
hash -r

export MISE_YES=1
export MISE_TRUSTED_CONFIG_PATHS="${MISE_TRUSTED_CONFIG_PATHS:-$PWD}"

if [ -z "${MISE_GITHUB_TOKEN:-}" ] && [ -z "${GITHUB_TOKEN:-}" ]; then
	if token="$(gh auth token 2>/dev/null)"; then
		export GITHUB_TOKEN="$token"
		export MISE_GITHUB_TOKEN="$token"
	fi
fi

eval "$(mise activate bash --shims)"
mise install
hk install --mise
