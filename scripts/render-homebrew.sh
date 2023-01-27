#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
RTX_VERSION=${RTX_VERSION#v*} \
	RTX_CHECKSUM_LINUX_X86_64=$(grep linux-x64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARM64=$(grep linux-arm64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_X86_64=$(grep macos-x64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_ARM64=$(grep macos-arm64.tar.xz "$RELEASE_DIR/SHASUMS256.txt" | cut -d ' ' -f1) \
	envsubst '$RTX_VERSION,$RTX_CHECKSUM_LINUX_X86_64,$RTX_CHECKSUM_LINUX_ARM64,$RTX_CHECKSUM_MACOS_X86_64,$RTX_CHECKSUM_MACOS_ARM64' \
	<rtx/packaging/homebrew/homebrew.rb
