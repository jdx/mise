#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-version.sh)

SHA512=$(curl -fsSL "https://github.com/jdx/mise/archive/$MISE_VERSION.tar.gz" | sha512sum | awk '{print $1}')

if [ ! -d aur ]; then
  git clone ssh://aur@aur.archlinux.org/mise.git aur
fi
git -C aur pull

cat >aur/PKGBUILD <<EOF
# Maintainer: Jeff Dickey <releases at mise dot jdx dot dev>

pkgname=mise
pkgver=${MISE_VERSION#v*}
pkgrel=1
pkgdesc='The front-end to your dev env'
arch=('x86_64')
url='https://github.com/jdx/mise'
license=('MIT')
makedepends=('cargo')
provides=('mise')
conflicts=('rtx' 'rtx-bin')
replaces=('rtx')
options=('!lto')
source=("\$pkgname-\$pkgver.tar.gz::https://github.com/jdx/\$pkgname/archive/v\$pkgver.tar.gz")
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
  install -Dm755 target/release/mise "\$pkgdir/usr/bin/mise"
  install -Dm644 man/man1/mise.1 "\$pkgdir/usr/share/man/man1/mise.1"
  install -Dm644 completions/mise.bash "\$pkgdir/usr/share/bash-completion/completions/mise"
  install -Dm644 completions/mise.fish "\$pkgdir/usr/share/fish/completions/mise.fish"
  install -Dm644 completions/_mise "\$pkgdir/usr/share/zsh/site-functions/_mise"
}

check() {
  cd "\$srcdir/\$pkgname-\$pkgver"
  ./target/release/mise --version
}
EOF

cat >aur/.SRCINFO <<EOF
pkgbase = mise
pkgdesc = The front-end to your dev env
pkgver = ${MISE_VERSION#v*}
pkgrel = 1
url = https://github.com/jdx/mise
arch = x86_64
license = MIT
makedepends = cargo
provides = mise
replaces = rtx
conflicts = rtx
conflicts = rtx-bin
source = mise-${MISE_VERSION#v*}.tar.gz::https://github.com/jdx/mise/archive/$MISE_VERSION.tar.gz
sha512sums = $SHA512

pkgname = mise
EOF

cd aur
git config user.name mise-en-dev
git config user.email 123107610+mise-en-dev@users.noreply.github.com
git add .SRCINFO PKGBUILD
if git diff-index --quiet HEAD --; then
  echo "No changes to PKGBUILD or .SRCINFO"
  exit 0
fi
git diff --cached
git commit -m "mise ${MISE_VERSION#v}"
if [ "$DRY_RUN" == 0 ]; then
  git push
fi
