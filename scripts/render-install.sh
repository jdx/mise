#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
RTX_VERSION=$RTX_VERSION \
  RTX_CHECKSUM_LINUX_X86_64=$(grep "rtx-v.*linux-x64.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_X86_64_MUSL=$(grep "rtx-v.*linux-x64-musl.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARM64=$(grep "rtx-v.*linux-arm64.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARM64_MUSL=$(grep "rtx-v.*linux-arm64-musl.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARMV6=$(grep "rtx-v.*linux-armv6.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARMV6_MUSL=$(grep "rtx-v.*linux-armv6-musl.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARMV7=$(grep "rtx-v.*linux-armv7.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_LINUX_ARMV7_MUSL=$(grep "rtx-v.*linux-armv7-musl.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_MACOS_X86_64=$(grep "rtx-v.*macos-x64.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  RTX_CHECKSUM_MACOS_ARM64=$(grep "rtx-v.*macos-arm64.tar.gz" "$RELEASE_DIR/$RTX_VERSION/SHASUMS256.txt") \
  envsubst '$RTX_VERSION,$RTX_CHECKSUM_LINUX_X86_64,$RTX_CHECKSUM_LINUX_ARM64,$RTX_CHECKSUM_MACOS_X86_64,$RTX_CHECKSUM_MACOS_ARM64' \
  <"$BASE_DIR/packaging/standalone/install.envsubst"
