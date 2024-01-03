#!/usr/bin/env bash
set -euxo pipefail

BASE_DIR="$(pwd)"
RELEASE_DIR="$(pwd)/tmp"
MISE_VERSION="v$(curl -fsSL https://mise.jdx.dev/VERSION)"
export BASE_DIR RELEASE_DIR MISE_VERSION

mkdir -p "$RELEASE_DIR/$MISE_VERSION"
curl -fsSL "https://mise.jdx.dev/$MISE_VERSION/SHASUMS256.txt" >"$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt"
./scripts/render-install.sh >tmp/install.sh
chmod +x tmp/install.sh
shellcheck tmp/install.sh

./tmp/install.sh
if [[ ! "$("$HOME/.local/bin/mise" -v)" =~ ^${MISE_VERSION//v/} ]]; then
  echo "mise version mismatch"
  exit 1
fi
rm -rf "$RELEASE_DIR"
