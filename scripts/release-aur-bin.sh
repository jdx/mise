#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-version.sh)

TAR_GZ_URI="https://github.com/jdx/mise/releases/download/$MISE_VERSION/mise-$MISE_VERSION-linux-x64.tar.gz"

SHA512=$(curl -fsSL "$TAR_GZ_URI" | sha512sum | awk '{print $1}')

if [ ! -d aur-bin ]; then
  git clone ssh://aur@aur.archlinux.org/mise-bin.git aur-bin
fi
git -C aur-bin pull

cat >aur-bin/PKGBUILD <<EOF
# Maintainer: Jeff Dickey <releases at mise dot jdx dot dev>

pkgname=mise-bin
pkgver=${MISE_VERSION#v*}
pkgrel=1
pkgdesc='The front-end to your dev env'
arch=('x86_64')
url='https://github.com/jdx/mise'
license=('MIT')
provides=('mise')
conflicts=('mise' 'rtx-bin' 'rtx')
replaces=('rtx-bin')
options=('!lto')
source=("mise-\$pkgver.tar.gz::${TAR_GZ_URI}")
sha512sums=('$SHA512')

build() {
  cd "\$srcdir/"
  mise/bin/mise completions bash > mise.bash
  mise/bin/mise completions fish > mise.fish
  mise/bin/mise completions zsh > _mise
}

package() {
  cd "\$srcdir/"
  install -Dm755 mise/bin/mise "\$pkgdir/usr/bin/mise"
  install -Dm644 mise/man/man1/mise.1 "\$pkgdir/usr/share/man/man1/mise.1"
  install -Dm644 mise.bash "\$pkgdir/usr/share/bash-completion/completions/mise"
  install -Dm644 mise.fish "\$pkgdir/usr/share/fish/completions/mise.fish"
  install -Dm644 _mise "\$pkgdir/usr/share/zsh/site-functions/_mise"
}

check() {
    "\$srcdir/mise/bin/mise" --version
}
EOF

cat >aur-bin/.SRCINFO <<EOF
pkgbase = mise-bin
pkgdesc = The front-end to your dev env
pkgver = ${MISE_VERSION#v*}
pkgrel = 1
url = https://github.com/jdx/mise
arch = x86_64
license = MIT
provides = mise
replaces = rtx-bin
conflicts = mise
conflicts = rtx-bin
conflicts = rtx
source = mise-${MISE_VERSION#v*}.tar.gz::${TAR_GZ_URI}
sha512sums = $SHA512

pkgname = mise-bin
EOF

cd aur-bin
git config user.name mise-en-dev
git config user.email 123107610+mise-en-dev@users.noreply.github.com
git add .SRCINFO PKGBUILD
if git diff-index --quiet HEAD --; then
  echo "No changes to PKGBUILD or .SRCINFO"
  exit 0
fi
git diff --cached
git commit -m "mise ${MISE_VERSION#v}"
if [[ "$DRY_RUN" == 0 ]]; then
  git push
fi
