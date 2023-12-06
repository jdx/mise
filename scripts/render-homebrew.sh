#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
RTX_VERSION=${RTX_VERSION#v*} \
	RTX_CHECKSUM_LINUX_X86_64=$(grep "rtx-v$RTX_VERSION-linux-x64.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_X86_64_MUSL=$(grep "rtx-v$RTX_VERSION-linux-x64-musl.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARM64=$(grep "rtx-v$RTX_VERSION-linux-arm64.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARM64_MUSL=$(grep "rtx-v$RTX_VERSION-linux-arm64-musl.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARMV6=$(grep "rtx-v$RTX_VERSION-linux-armv6.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARMV6_MUSL=$(grep "rtx-v$RTX_VERSION-linux-armv6-musl.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARMV7=$(grep "rtx-v$RTX_VERSION-linux-armv7.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_LINUX_ARMV7_MUSL=$(grep "rtx-v$RTX_VERSION-linux-armv7-musl.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_X86_64=$(grep "rtx-v$RTX_VERSION-macos-x64.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	RTX_CHECKSUM_MACOS_ARM64=$(grep "rtx-v$RTX_VERSION-macos-arm64.tar.xz" "$RELEASE_DIR/v$RTX_VERSION/SHASUMS256.txt" | cut -d ' ' -f1) \
	envsubst '$RTX_VERSION,$RTX_CHECKSUM_LINUX_X86_64,$RTX_CHECKSUM_LINUX_ARM64,$RTX_CHECKSUM_MACOS_X86_64,$RTX_CHECKSUM_MACOS_ARM64' \
	<rtx/packaging/homebrew/homebrew.rb
