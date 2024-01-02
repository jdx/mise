#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-version.sh)

mkdir -p mise/lib
touch mise/lib/.disable-self-update

tar -xvJf "dist/mise-$MISE_VERSION-linux-x64.tar.xz"
fpm -s dir -t rpm \
  --name mise \
  --license MIT \
  --version "${MISE_VERSION#v*}" \
  --architecture x86_64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/mise" \
  --maintainer "Jeff Dickey @jdx" \
  mise/bin/mise=/usr/bin/mise \
  mise/lib/.disable-self-update=/usr/lib/mise/.disable-self-update \
  mise/man/man1/mise.1=/usr/share/man/man1/mise.1

tar -xvJf "dist/mise-$MISE_VERSION-linux-arm64.tar.xz"
fpm -s dir -t rpm \
  --name mise \
  --license MIT \
  --version "${MISE_VERSION#v*}" \
  --architecture aarch64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/mise" \
  --maintainer "Jeff Dickey @jdx" \
  mise/bin/mise=/usr/bin/mise \
  mise/lib/.disable-self-update=/usr/lib/mise/.disable-self-update \
  mise/man/man1/mise.1=/usr/share/man/man1/mise.1

cat <<EOF >~/.rpmmacros
%_signature gpg
%_gpg_name 8B81C9D17413A06D
EOF

mkdir -p dist/rpmrepo/packages
cp -v packaging/rpm/mise.repo dist/rpmrepo
cp -v ./*.rpm dist/rpmrepo/packages
rpm --addsign dist/rpmrepo/packages/*.rpm
createrepo dist/rpmrepo
gpg --batch --yes --detach-sign --armor dist/rpmrepo/repodata/repomd.xml
