#!/usr/bin/env bash
set -euxo pipefail

# shellcheck disable=SC2016
MISE_CURRENT_VERSION=$MISE_VERSION \
  MISE_CHECKSUM_LINUX_X86_64=$(grep "mise-v.*linux-x64.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_X86_64_MUSL=$(grep "mise-v.*linux-x64-musl.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARM64=$(grep "mise-v.*linux-arm64.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARM64_MUSL=$(grep "mise-v.*linux-arm64-musl.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARMV7=$(grep "mise-v.*linux-armv7.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARMV7_MUSL=$(grep "mise-v.*linux-armv7-musl.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_MACOS_X86_64=$(grep "mise-v.*macos-x64.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_MACOS_ARM64=$(grep "mise-v.*macos-arm64.tar.gz" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_X86_64_ZSTD=$(grep "mise-v.*linux-x64.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_X86_64_MUSL_ZSTD=$(grep "mise-v.*linux-x64-musl.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARM64_ZSTD=$(grep "mise-v.*linux-arm64.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARM64_MUSL_ZSTD=$(grep "mise-v.*linux-arm64-musl.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARMV7_ZSTD=$(grep "mise-v.*linux-armv7.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_LINUX_ARMV7_MUSL_ZSTD=$(grep "mise-v.*linux-armv7-musl.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_MACOS_X86_64_ZSTD=$(grep "mise-v.*macos-x64.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  MISE_CHECKSUM_MACOS_ARM64_ZSTD=$(grep "mise-v.*macos-arm64.tar.zst" "$RELEASE_DIR/$MISE_VERSION/SHASUMS256.txt") \
  envsubst '$MISE_CURRENT_VERSION,$MISE_CHECKSUM_LINUX_X86_64,$MISE_CHECKSUM_LINUX_X86_64_MUSL,$MISE_CHECKSUM_LINUX_ARM64,$MISE_CHECKSUM_LINUX_ARM64_MUSL,$MISE_CHECKSUM_LINUX_ARMV6,$MISE_CHECKSUM_LINUX_ARMV6_MUSL,$MISE_CHECKSUM_LINUX_ARMV7,$MISE_CHECKSUM_LINUX_ARMV7_MUSL,$MISE_CHECKSUM_MACOS_X86_64,$MISE_CHECKSUM_MACOS_ARM64,$MISE_CHECKSUM_LINUX_X86_64_ZSTD,$MISE_CHECKSUM_LINUX_X86_64_MUSL_ZSTD,$MISE_CHECKSUM_LINUX_ARM64_ZSTD,$MISE_CHECKSUM_LINUX_ARM64_MUSL_ZSTD,$MISE_CHECKSUM_LINUX_ARMV7_ZSTD,$MISE_CHECKSUM_LINUX_ARMV7_MUSL_ZSTD,$MISE_CHECKSUM_MACOS_X86_64_ZSTD,$MISE_CHECKSUM_MACOS_ARM64_ZSTD' \
  <"$BASE_DIR/packaging/standalone/install.envsubst"
