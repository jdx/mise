#!/usr/bin/env bash
set -euxo pipefail

if [ -z "$ZIPSIGN" ]; then
  echo "ZIPSIGN is not defined"
  exit 0
fi

if ! command -v zipsign >/dev/null 2>&1; then
  cargo install zipsign
fi

mkdir -p ~/.zipsign
echo "$ZIPSIGN" | base64 -d >~/.zipsign/mise.priv
