#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

mkdir -p rtx/lib
touch rtx/lib/.disable-self-update

tar -xvJf "dist/rtx-$RTX_VERSION-linux-x64.tar.xz"
fpm -s dir -t rpm \
  --name rtx \
  --license MIT \
  --version "${RTX_VERSION#v*}" \
  --architecture x86_64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/rtx" \
  --maintainer "Jeff Dickey @jdx" \
  rtx/bin/rtx=/usr/bin/rtx \
  rtx/lib/.disable-self-update=/usr/lib/rtx/.disable-self-update \
  rtx/man/man1/rtx.1=/usr/share/man/man1/rtx.1

tar -xvJf "dist/rtx-$RTX_VERSION-linux-arm64.tar.xz"
fpm -s dir -t rpm \
  --name rtx \
  --license MIT \
  --version "${RTX_VERSION#v*}" \
  --architecture aarch64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/rtx" \
  --maintainer "Jeff Dickey @jdx" \
  rtx/bin/rtx=/usr/bin/rtx \
  rtx/lib/.disable-self-update=/usr/lib/rtx/.disable-self-update \
  rtx/man/man1/rtx.1=/usr/share/man/man1/rtx.1

cat <<EOF >~/.rpmmacros
%_signature gpg
%_gpg_name 408B88DB29DDE9E0
EOF

mkdir -p dist/rpmrepo/packages
cp -v packaging/rpm/rtx.repo dist/rpmrepo
cp -v ./*.rpm dist/rpmrepo/packages
rpm --addsign dist/rpmrepo/packages/*.rpm
createrepo dist/rpmrepo
gpg --batch --yes --detach-sign --armor dist/rpmrepo/repodata/repomd.xml
