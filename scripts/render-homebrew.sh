#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
MISE_VERSION=${MISE_VERSION#v*} \
	MISE_CHECKSUM_LINUX_X86_64=$(grep "mise-v$MISE_VERSION-linux-x64.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_X86_64_MUSL=$(grep "mise-v$MISE_VERSION-linux-x64-musl.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARM64=$(grep "mise-v$MISE_VERSION-linux-arm64.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARM64_MUSL=$(grep "mise-v$MISE_VERSION-linux-arm64-musl.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARMV6=$(grep "mise-v$MISE_VERSION-linux-armv6.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARMV6_MUSL=$(grep "mise-v$MISE_VERSION-linux-armv6-musl.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARMV7=$(grep "mise-v$MISE_VERSION-linux-armv7.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_LINUX_ARMV7_MUSL=$(grep "mise-v$MISE_VERSION-linux-armv7-musl.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_MACOS_X86_64=$(grep "mise-v$MISE_VERSION-macos-x64.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	MISE_CHECKSUM_MACOS_ARM64=$(grep "mise-v$MISE_VERSION-macos-arm64.tar.xz" "$RELEASE_DIR/v$MISE_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	envsubst '$MISE_VERSION,$MISE_CHECKSUM_LINUX_X86_64,$MISE_CHECKSUM_LINUX_ARM64,$MISE_CHECKSUM_MACOS_X86_64,$MISE_CHECKSUM_MACOS_ARM64' \
	<mise/packaging/homebrew/homebrew.rb
