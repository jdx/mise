#!/usr/bin/env bash
set -euxo pipefail

VERSION=$(curl https://rtx.jdxcode.com/VERSION | sed -e "s/^v//")
SHA512=$(curl -L "https://github.com/jdxcode/rtx/archive/v$VERSION.tar.gz" | sha512sum | awk '{print $1}')

if [ ! -d aur ]; then
  git clone ssh://aur@aur.archlinux.org/rtx.git aur
fi

cat >aur/PKGBUILD <<EOF
# Maintainer: Jeff Dickey <releases at chim dot sh>

pkgname=rtx
pkgver=$VERSION
pkgrel=1
pkgdesc='Polyglot runtime manager'
arch=('x86_64')
url='https://github.com/jdxcode/rtx'
license=('MIT')
makedepends=('cargo' 'jq')
provides=('rtx')
conflicts=('rtx')
source=("\$pkgname-\$pkgver.tar.gz::https://github.com/jdxcode/\$pkgname/archive/v\$pkgver.tar.gz")
sha512sums=('$SHA512')

prepare() {
    cd "\$pkgname-\$pkgver"

    cargo fetch --locked --target "\$CARCH-unknown-linux-gnu"
}

build() {
    cd "\$pkgname-\$pkgver"

    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --release --locked --message-format=json-render-diagnostics |
      jq -r 'select(.out_dir) | select(.package_id | startswith("ripgrep ")) | .out_dir' > out_dir
}

package() {
    cd "\$pkgname-\$pkgver"
    local OUT_DIR=\$(<out_dir)

    install -Dm755 "target/release/\$pkgname" -t "\$pkgdir/usr/bin"

    install -Dm644 "README.md" "\$pkgdir/usr/share/doc/\$pkgname/README.md"
    install -Dm644 "LICENSE" "\$pkgdir/usr/share/licenses/\$pkgname/LICENSE"
}

check() {
    cd "\$pkgname-\$pkgver"

    export RUSTUP_TOOLCHAIN=stable
    cargo test --locked
}
EOF

cat >aur/.SRCINFO <<EOF
pkgbase = rtx
	pkgdesc = Polyglot runtime manager
	pkgver = $VERSION
	pkgrel = 1
	url = https://github.com/jdxcode/rtx
	arch = x86_64
	license = MIT
	makedepends = cargo
	makedepends = jq
	provides = rtx
	conflicts = rtx
	source = rtx-$VERSION.tar.gz::https://github.com/jdxcode/rtx/archive/v$VERSION.tar.gz
	sha512sums = $SHA512

pkgname = rtx
EOF

cd aur
git add .SRCINFO PKGBUILD
git commit -m "rtx $VERSION"
git push
