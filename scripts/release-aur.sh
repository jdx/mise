#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

SHA512=$(curl -L "https://github.com/jdxcode/rtx/archive/$RTX_VERSION.tar.gz" | sha512sum | awk '{print $1}')

if [ ! -d aur ]; then
  git clone ssh://aur@aur.archlinux.org/rtx.git aur
fi
git -C aur pull

cat >aur/PKGBUILD <<EOF
# Maintainer: Jeff Dickey <releases at rtx dot pub>

pkgname=rtx
pkgver=${RTX_VERSION#v*}
pkgrel=1
pkgdesc='Polyglot runtime manager'
arch=('x86_64')
url='https://github.com/jdxcode/rtx'
license=('MIT')
makedepends=('cargo')
provides=('rtx')
conflicts=('rtx')
source=("\$pkgname-\$pkgver.tar.gz::https://github.com/jdxcode/\$pkgname/archive/v\$pkgver.tar.gz")
sha512sums=('$SHA512')

prepare() {
    cd "\$srcdir/\$pkgname-\$pkgver"
    cargo fetch --locked --target "\$CARCH-unknown-linux-gnu"
}

build() {
    cd "\$srcdir/\$pkgname-\$pkgver"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release
}

package() {
    cd "\$srcdir/\$pkgname-\$pkgver"
    install -Dm0755 -t "\$pkgdir/usr/bin/" "target/release/\$pkgname"
}

check() {
    cd "\$srcdir/\$pkgname-\$pkgver"
    ./target/release/rtx --version
}
EOF

cat >aur/.SRCINFO <<EOF
pkgbase = rtx
	pkgdesc = Polyglot runtime manager
	pkgver = ${RTX_VERSION#v*}
	pkgrel = 1
	url = https://github.com/jdxcode/rtx
	arch = x86_64
	license = MIT
	makedepends = cargo
	provides = rtx
	conflicts = rtx
	source = rtx-${RTX_VERSION#v*}.tar.gz::https://github.com/jdxcode/rtx/archive/$RTX_VERSION.tar.gz
	sha512sums = $SHA512

pkgname = rtx
EOF

cd aur
git add .SRCINFO PKGBUILD
git commit -m "rtx $RTX_VERSION"
git push
