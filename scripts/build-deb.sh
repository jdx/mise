#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

tar -xvJf "dist/rtx-deb-$RTX_VERSION-linux-x64.tar.xz"
fpm -s dir -t deb \
	--name rtx \
	--license MIT \
	--version "${RTX_VERSION#v*}" \
	--architecture amd64 \
	--description "Polyglot runtime manager" \
	--url "https://github.com/jdxcode/rtx" \
	--maintainer "Jeff Dickey @jdxcode" \
	rtx/bin/rtx=/usr/bin/rtx

tar -xvJf "dist/rtx-deb-$RTX_VERSION-linux-arm64.tar.xz"
fpm -s dir -t deb \
	--name rtx \
	--license MIT \
	--version "${RTX_VERSION#v*}" \
	--architecture arm64 \
	--description "Polyglot runtime manager" \
	--url "https://github.com/jdxcode/rtx" \
	--maintainer "Jeff Dickey @jdxcode" \
	rtx/bin/rtx=/usr/bin/rtx

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
gpg -u 408B88DB29DDE9E0 -abs <Release >Release.gpg
gpg -u 408B88DB29DDE9E0 -abs --clearsign <Release >InRelease
cd "$GITHUB_WORKSPACE"
