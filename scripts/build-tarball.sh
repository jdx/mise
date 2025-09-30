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

if [[ $os == "linux" ]]; then
	cross build --profile=serious --target "$RUST_TRIPLE" --no-default-features --features "$features"
else
	cargo build --profile=serious --target "$RUST_TRIPLE" --no-default-features --features "$features"
fi

# Check glibc compatibility for x86_64-unknown-linux-gnu (Amazon Linux 2 requirement: glibc <= 2.26)
if [[ $RUST_TRIPLE == "x86_64-unknown-linux-gnu" ]]; then
	echo "Checking glibc compatibility for Amazon Linux 2..."
	# Use CARGO_TARGET_DIR if set, otherwise default to target
	target_dir="${CARGO_TARGET_DIR:-target}"
	binary_path="$target_dir/$RUST_TRIPLE/serious/mise"
	if [[ -f $binary_path ]]; then
		max_glibc=$(objdump -p "$binary_path" | grep 'GLIBC_' | sed 's/.*GLIBC_//' | sort -V | tail -1)
		echo "Maximum glibc version required: $max_glibc"

		# Amazon Linux 2 has glibc 2.26, so we check if our binary requires <= 2.26
		if printf '%s\n' "$max_glibc" "2.26" | sort -V -C; then
			echo "✅ Binary is compatible with Amazon Linux 2 (glibc $max_glibc <= 2.26)"
		else
			echo "❌ Binary requires glibc $max_glibc, which is newer than Amazon Linux 2's glibc 2.26"
			echo "This binary will NOT work on Amazon Linux 2"
			exit 1
		fi
	else
		echo "Warning: Binary not found at $binary_path, skipping glibc check"
	fi
fi
mkdir -p dist/mise/bin
mkdir -p dist/mise/man/man1
mkdir -p dist/mise/share/fish/vendor_conf.d
# Use CARGO_TARGET_DIR if set, otherwise default to target
target_dir="${CARGO_TARGET_DIR:-target}"
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
	XZ_OPT=-9 tar -acf "$basename.tar.xz" mise
	tar -cf - mise | gzip -9 >"$basename.tar.gz"
	ZSTD_NBTHREADS=0 ZSTD_CLEVEL=19 tar -acf "$basename.tar.zst" mise
	ls -oh "$basename.tar."*
fi
