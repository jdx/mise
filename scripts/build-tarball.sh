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

features="rustls-native-roots,self_update,vfox/vendored-lua,openssl/vendored"
if [[ $os == "linux" ]] && [[ $arch == "armv7" ]]; then
	features="$features,aws-lc-rs"
fi

if [[ $os == "macos" ]]; then
	# Targeting macOS 12+ makes ld emit chained fixups (LC_DYLD_CHAINED_FIXUPS),
	# which dyld applies lazily per page instead of eagerly interpreting ~170k
	# legacy rebase/bind opcodes on every launch. Measurably faster startup for
	# a binary this large. macOS 11 (EOL since 2023) cannot run these binaries.
	export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-12.0}
fi

if [[ $os == "linux" ]]; then
	cross build --profile=serious --target "$RUST_TRIPLE" --no-default-features --features "$features"
else
	cargo build --profile=serious --target "$RUST_TRIPLE" --no-default-features --features "$features"
fi

# Use CARGO_TARGET_DIR if set, otherwise default to target
target_dir="${CARGO_TARGET_DIR:-target}"
binary_path="$target_dir/$RUST_TRIPLE/serious/mise"

case "$RUST_TRIPLE" in
x86_64-unknown-linux-gnu)
	echo "Checking glibc compatibility for Amazon Linux 2..."
	scripts/check-glibc.sh "$binary_path" "2.26" "Amazon Linux 2"
	;;
aarch64-unknown-linux-gnu)
	echo "Checking glibc compatibility for Amazon Linux 2023..."
	scripts/check-glibc.sh "$binary_path" "2.34" "Amazon Linux 2023"
	;;
esac
mkdir -p dist/mise/bin
mkdir -p dist/mise/man/man1
mkdir -p dist/mise/share/fish/vendor_conf.d
cp "$target_dir/$RUST_TRIPLE/serious/mise"* dist/mise/bin
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
	XZ_OPT=-9 tar --owner=0 --group=0 -acf "$basename.tar.xz" mise
	tar --owner=0 --group=0 -cf - mise | gzip -9 >"$basename.tar.gz"
	ZSTD_NBTHREADS=0 ZSTD_CLEVEL=19 tar --owner=0 --group=0 -acf "$basename.tar.zst" mise
	ls -oh "$basename.tar."*
fi
