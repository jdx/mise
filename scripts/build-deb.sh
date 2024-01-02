#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-version.sh)

mkdir -p mise/lib
touch mise/lib/.disable-self-update

tar -xvJf "dist/mise-$MISE_VERSION-linux-x64.tar.xz"
fpm -s dir -t deb \
  --name mise \
  --license MIT \
  --version "${MISE_VERSION#v*}" \
  --architecture amd64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/mise" \
  --maintainer "Jeff Dickey @jdx" \
  mise/bin/mise=/usr/bin/mise \
  mise/lib/.disable-self-update=/usr/lib/mise/.disable-self-update \
  mise/man/man1/mise.1=/usr/share/man/man1/mise.1

tar -xvJf "dist/mise-$MISE_VERSION-linux-arm64.tar.xz"
fpm -s dir -t deb \
  --name mise \
  --license MIT \
  --version "${MISE_VERSION#v*}" \
  --architecture arm64 \
  --description "The front-end to your dev env" \
  --url "https://github.com/jdx/mise" \
  --maintainer "Jeff Dickey @jdx" \
  mise/bin/mise=/usr/bin/mise \
  mise/lib/.disable-self-update=/usr/lib/mise/.disable-self-update \
  mise/man/man1/mise.1=/usr/share/man/man1/mise.1

mkdir -p dist/deb/pool/main
cp -v ./*.deb dist/deb/pool/main
mkdir -p dist/deb/dists/stable/main/binary-amd64
mkdir -p dist/deb/dists/stable/main/binary-arm64
cd dist/deb
dpkg-scanpackages --arch amd64 pool/ >dists/stable/main/binary-amd64/Packages
dpkg-scanpackages --arch arm64 pool/ >dists/stable/main/binary-arm64/Packages
gzip -9c <dists/stable/main/binary-amd64/Packages >dists/stable/main/binary-amd64/Packages.gz
gzip -9c <dists/stable/main/binary-arm64/Packages >dists/stable/main/binary-arm64/Packages.gz
cd ../..

cd dist/deb/dists/stable
"$GITHUB_WORKSPACE/packaging/deb/generate-release.sh" >Release
gpg -u 8B81C9D17413A06D -abs <Release >Release.gpg
gpg -u 8B81C9D17413A06D -abs --clearsign <Release >InRelease
cd "$GITHUB_WORKSPACE"
