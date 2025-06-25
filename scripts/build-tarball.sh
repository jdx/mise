#!/usr/bin/env bash
set -euo pipefail

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
	*-windows-*)
		echo "windows"
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
	arm*)
		echo "armv7"
		;;
	x86_64-*)
		echo "x64"
		;;
	universal2-*)
		echo "universal"
		;;
	*)
		error "unsupported arch: $RUST_TRIPLE"
		;;
	esac
}
get_suffix() {
	case "$RUST_TRIPLE" in
	*-musl | *-musleabi | *-musleabihf)
		echo "-musl"
		;;
	*)
		echo ""
		;;
	esac
}
#endregion

set -x
os=$(get_os)
arch=$(get_arch)
suffix=$(get_suffix)
version=$(./scripts/get-version.sh)
basename=mise-$version-$os-$arch$suffix

case "$os-$arch" in
linux-arm*)
	# don't use sccache
	unset RUSTC_WRAPPER
	;;
esac

if command -v cross >/dev/null; then
	cross build --profile=serious --target "$RUST_TRIPLE" --features openssl/vendored
elif command -v zig >/dev/null; then
	cargo zigbuild --profile=serious --target "$RUST_TRIPLE" --features openssl/vendored
else
	cargo build --profile=serious --target "$RUST_TRIPLE" --features openssl/vendored
fi
mkdir -p dist/mise/bin
mkdir -p dist/mise/man/man1
mkdir -p dist/mise/share/fish/vendor_conf.d
cp "target/$RUST_TRIPLE/serious/mise"* dist/mise/bin
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE

if [[ $os != "windows" ]]; then
	cp {,dist/mise/}man/man1/mise.1
	cp {,dist/mise/}share/fish/vendor_conf.d/mise-activate.fish
fi

cd dist

if [[ $os == "macos" ]]; then
	codesign -f --prefix dev.jdx. -s "Developer ID Application: Jeffrey Dickey (4993Y37DX6)" mise/bin/mise
fi

if [[ $os == "windows" ]]; then
	zip -r "$basename.zip" mise
	ls -oh "$basename.zip"
else
	XZ_OPT=-9 tar -acf "$basename.tar.xz" mise
	tar -cf - mise | gzip -9 >"$basename.tar.gz"
	ZSTD_NBTHREADS=0 ZSTD_CLEVEL=19 tar -acf "$basename.tar.zst" mise
	ls -oh "$basename.tar."*
fi
