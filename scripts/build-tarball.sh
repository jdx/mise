#!/usr/bin/env bash
set -euxo pipefail

error() {
	echo "$@" >&2
	exit 1
}

RUST_TRIPLE=${1:-$(rustc -vV | grep ^host: | cut -d ' ' -f2)}

#region os/arch
get_os() {
	case "$RUST_TRIPLE" in
	*-apple-darwin*)
		echo "macos"
		;;
	*-linux-*)
		echo "linux"
		;;
	*)
		error "unsupported OS: $RUST_TRIPLE"
		;;
	esac
}

get_arch() {
	case "$RUST_TRIPLE" in
	aarch64-*)
		echo "arm64"
		;;
	x86_64-*)
		echo "x64"
		;;
	*)
		error "unsupported arch: $RUST_TRIPLE"
		;;
	esac
}
#endregion

VERSION=$(./scripts/get-version.sh)
BASENAME=rtx-$VERSION-$(get_os)-$(get_arch)

#if [ "${CROSS:-}" = "1" ]; then
#  cross build --release --target "$RUST_TRIPLE"
#else
#  cargo build --release --target "$RUST_TRIPLE"
#fi

mkdir -p "dist/rtx/bin"
cp "target/$RUST_TRIPLE/release/rtx" "dist/rtx/bin/rtx"
cp README.md "dist/rtx/README.md"

cd dist
tar -cJf "$BASENAME.tar.xz" rtx
tar -czf "$BASENAME.tar.gz" rtx

ls -oh "$BASENAME.tar.xz"
