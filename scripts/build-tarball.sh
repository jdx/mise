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
    arm-*)
      echo "armv6"
      ;;
    armv7-*)
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
VERSION=$(./scripts/get-version.sh)
BASENAME=$NAME-$VERSION-$(get_os)-$(get_arch)$(get_suffix)

if command -v cross >/dev/null; then
  cross build --profile=serious "$@"
elif command -v zig >/dev/null; then
  cargo zigbuild --profile=serious "$@"
else
  cargo build --profile=serious "$@"
fi
mkdir -p dist/mise/bin
mkdir -p dist/mise/man/man1
mkdir -p dist/mise/share/fish/vendor_conf.d
cp "target/$RUST_TRIPLE/serious/mise" dist/mise/bin/mise
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
cp {,dist/mise/}man/man1/mise.1
cp {,dist/mise/}share/fish/vendor_conf.d/mise-activate.fish

cd dist
tar -cJf "$BASENAME.tar.xz" mise
tar -czf "$BASENAME.tar.gz" mise

if [ -f ~/.zipsign/mise.priv ]; then
  zipsign sign tar "$BASENAME.tar.gz" ~/.zipsign/mise.priv
  zipsign verify tar "$BASENAME.tar.gz" ../zipsign.pub
fi

ls -oh "$BASENAME.tar.xz"
