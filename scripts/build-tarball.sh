#!/usr/bin/env bash
set -euo pipefail

error() {
	echo "$@" >&2
	exit 1
}

NAME="$1"
shift

for arg in "$@"; do
	if [ "${next_target:-}" = 1 ]; then
		next_target=
		TARGET="$arg"
		continue
	fi
	case "$arg" in
	--target)
		next_target=1
		;;
	*) ;;

	esac
done

RUST_TRIPLE=${TARGET:-$(rustc -vV | grep ^host: | cut -d ' ' -f2)}
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

set -x
VERSION=$(./scripts/get-version.sh)
BASENAME=$NAME-$VERSION-$(get_os)-$(get_arch)

if [ "${CROSS:-}" = "1" ]; then
	cross build "$@"
else
	cargo build "$@"
fi
mkdir -p dist/rtx/bin
mkdir -p dist/rtx/man/man1
mkdir -p dist/rtx/share/fish/vendor_conf.d
cp "target/$RUST_TRIPLE/release/rtx" dist/rtx/bin/rtx
cp README.md dist/rtx/README.md
cp LICENSE dist/rtx/LICENSE
cp {,dist/rtx/}man/man1/rtx.1
cp {,dist/rtx/}share/fish/vendor_conf.d/rtx-activate.fish

cd dist
tar -cJf "$BASENAME.tar.xz" rtx
tar -czf "$BASENAME.tar.gz" rtx

if [ -f ~/.zipsign/rtx.priv ]; then
	zipsign sign tar "$BASENAME.tar.gz" ~/.zipsign/rtx.priv
	zipsign verify tar "$BASENAME.tar.gz" ../zipsign.pub
fi

ls -oh "$BASENAME.tar.xz"
