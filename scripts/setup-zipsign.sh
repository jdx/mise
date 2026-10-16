#!/usr/bin/env bash
set -euxo pipefail

if [ -z "$ZIPSIGN" ]; then
	echo "ZIPSIGN is not defined"
	exit 0
fi

if ! command -v zipsign >/dev/null 2>&1; then
	# --locked: every zip release zipsign 0.2.1 can resolve against has been
	# yanked, so a fresh resolution fails; the published Cargo.lock still resolves.
	cargo install zipsign --locked
fi

mkdir -p ~/.zipsign
echo "$ZIPSIGN" | base64 -d >~/.zipsign/mise.priv
