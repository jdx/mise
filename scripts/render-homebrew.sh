#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
RTX_VERSION=${RTX_VERSION#v*} \
	RTX_CHECKSUM_LINUX_X86_64=$(grep linux-brew-x64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARM64=$(grep linux-brew-arm64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_X86_64=$(grep macos-brew-x64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_ARM64=$(grep macos-brew-arm64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	envsubst '$RTX_VERSION,$RTX_CHECKSUM_LINUX_X86_64,$RTX_CHECKSUM_LINUX_ARM64,$RTX_CHECKSUM_MACOS_X86_64,$RTX_CHECKSUM_MACOS_ARM64' \
	<rtx/packaging/homebrew/homebrew.rb
