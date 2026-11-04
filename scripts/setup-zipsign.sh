#!/usr/bin/env bash
set -euxo pipefail

if [ -z "$ZIPSIGN" ]; then
	echo "ZIPSIGN is not defined"
	exit 0
fi

if ! command -v zipsign >/dev/null 2>&1; then
	# --locked: build against the Cargo.lock zipsign published, not a fresh resolution
	cargo install zipsign --version 0.2.2 --locked
fi

mkdir -p ~/.zipsign
echo "$ZIPSIGN" | base64 -d >~/.zipsign/mise.priv
