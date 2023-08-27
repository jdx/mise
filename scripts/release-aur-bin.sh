#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

TAR_GZ_URI="https://github.com/jdx/rtx/releases/download/${RTX_VERSION}/rtx-${RTX_VERSION}-linux-x64.tar.gz"

SHA512=$(curl -L "$TAR_GZ_URI" | sha512sum | awk '{print $1}')

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
url='https://github.com/jdx/rtx'
license=('MIT')
provides=('rtx')
conflicts=('rtx')
options=('!lto')
source=("rtx-\$pkgver.tar.gz::${TAR_GZ_URI}")
sha512sums=('$SHA512')

build() {
 cd "\$srcdir/"
 rtx/bin/rtx completions bash > rtx.bash
 rtx/bin/rtx completions fish > rtx.fish
 rtx/bin/rtx completions zsh > _rtx
}

package() {
    cd "\$srcdir/"
    install -Dm755 rtx/bin/rtx "\$pkgdir/usr/bin/rtx"
    install -Dm644 rtx/man/man1/rtx.1 "\$pkgdir/usr/share/man/man1/rtx.1"
    install -Dm644 rtx.bash "\$pkgdir/usr/share/bash-completion/completions/rtx"
    install -Dm644 rtx.fish "\$pkgdir/usr/share/fish/completions/rtx.fish"
    install -Dm644 _rtx "\$pkgdir/usr/share/zsh/site-functions/_rtx"
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
	url = https://github.com/jdx/rtx
	arch = x86_64
	license = MIT
	provides = rtx
	conflicts = rtx
	source = rtx-${RTX_VERSION#v*}.tar.gz::${TAR_GZ_URI}
	sha512sums = $SHA512

pkgname = rtx-bin
EOF

cd aur-bin
git add .SRCINFO PKGBUILD
git commit -m "rtx ${RTX_VERSION#v}"
git push
