#!/usr/bin/env bash
set -euxo pipefail

if [ -z "$ZIPSIGN" ]; then
	echo "ZIPSIGN is not defined"
	exit 0
fi

cargo install zipsign
mkdir -p ~/.zipsign
echo "$ZIPSIGN" | base64 -d >~/.zipsign/rtx.priv
