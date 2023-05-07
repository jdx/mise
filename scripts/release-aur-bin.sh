#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

SHA512=$(curl -L "https://github.com/jdxcode/rtx/archive/$RTX_VERSION.tar.gz" | sha512sum | awk '{print $1}')

if [ ! -d aur-bin ]; then
	git clone ssh://aur@aur.archlinux.org/rtx-bin.git aur-bin
fi
git -C aur-bin pull

cat >aur-bin/PKGBUILD <<EOF
# Maintainer: Jeff Dickey <releases at rtx dot pub>

pkgname=rtx-bin
pkgver=${RTX_VERSION#v*}
pkgrel=1
pkgdesc='Polyglot runtime manager'
arch=('x86_64')
url='https://github.com/jdxcode/rtx'
license=('MIT')
provides=('rtx')
conflicts=('rtx')
options=('!lto')
source=("\$pkgname-\$pkgver.tar.gz::https://github.com/jdxcode/\$pkgname/archive/v\$pkgver.tar.gz")
sha512sums=('$SHA512')

prepare() {
    tar -xzf rtx-v\$pkgver-linux-x64.tar.gz
}

package() {
    cd "\$srcdir/"
    install -Dm755 rtx/bin/rtx "\$pkgdir/usr/bin/rtx"
    install -Dm644 rtx/man/man1/rtx.1 "\$pkgdir/usr/share/man/man1/rtx.1"
}

check() {
    "\$srcdir/rtx/bin/rtx" --version
}
EOF

cat >aur-bin/.SRCINFO <<EOF
pkgbase = rtx-bin
	pkgdesc = Polyglot runtime manager
	pkgver = ${RTX_VERSION#v*}
	pkgrel = 1
	url = https://github.com/jdxcode/rtx
	arch = x86_64
	license = MIT
	provides = rtx
	conflicts = rtx
	source = rtx-${RTX_VERSION#v*}.tar.gz::https://github.com/jdxcode/rtx/archive/$RTX_VERSION.tar.gz
	sha512sums = $SHA512

pkgname = rtx-bin
EOF

cd aur-bin
git add .SRCINFO PKGBUILD
git commit -m "rtx ${RTX_VERSION#v}"
git push
