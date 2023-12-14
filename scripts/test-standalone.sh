#!/usr/bin/env bash
set -euxo pipefail

BASE_DIR="$(pwd)"
RELEASE_DIR="$(pwd)/tmp"
RTX_VERSION="v$(curl -fsSL https://rtx.jdx.dev/VERSION)"
export BASE_DIR RELEASE_DIR RTX_VERSION

mkdir -p "$RELEASE_DIR/$RTX_VERSION"
curl -fsSL "https://rtx.jdx.dev/$RTX_VERSION/SHASUMS256.txt" >"$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt"
./scripts/render-install.sh >tmp/install.sh
chmod +x tmp/install.sh
shellcheck tmp/install.sh

RTX_DATA_DIR="$RELEASE_DIR" ./tmp/install.sh
if [[ ! "$("$RELEASE_DIR/bin/rtx" -v)" =~ ^${RTX_VERSION//v/} ]]; then
  echo "rtx version mismatch"
  exit 1
fi
rm -rf "$RELEASE_DIR"
