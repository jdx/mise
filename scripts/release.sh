#!/usr/bin/env bash
set -euxo pipefail

git config --global user.name rtx-vm
git config --global user.email 123107610+rtx-vm@users.noreply.github.com

RTX_VERSION=$(cd rtx && ./scripts/get-version.sh)
RELEASE_DIR=releases
export RTX_VERSION RELEASE_DIR
rm -rf "${RELEASE_DIR:?}/$RTX_VERSION"
mkdir -p "$RELEASE_DIR/$RTX_VERSION"

#cp artifacts/tarball-x86_64-pc-windows-gnu/*.zip "$RELEASE_DIR/$RTX_VERSION"
#cp artifacts/tarball-x86_64-pc-windows-gnu/*.zip "$RELEASE_DIR/rtx-latest-windows.zip"

targets=(
	x86_64-unknown-linux-gnu
	aarch64-unknown-linux-gnu
	x86_64-apple-darwin
	aarch64-apple-darwin
)
for target in "${targets[@]}"; do
	cp "artifacts/tarball-$target/"*.tar.gz "$RELEASE_DIR/$RTX_VERSION"
	cp "artifacts/tarball-$target/"*.tar.xz "$RELEASE_DIR/$RTX_VERSION"
done

platforms=(
	linux-x64
	linux-arm64
	macos-x64
	macos-arm64
)
for platform in "${platforms[@]}"; do
	cp "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform.tar.gz" "$RELEASE_DIR/rtx-latest-$platform.tar.gz"
	cp "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform.tar.xz" "$RELEASE_DIR/rtx-latest-$platform.tar.xz"
	tar -xvzf "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform.tar.gz"
  cp -v rtx/bin/rtx "$RELEASE_DIR/rtx-latest-$platform"
  cp -v rtx/bin/rtx "$RELEASE_DIR/$RTX_VERSION/rtx-$RTX_VERSION-$platform"
done

pushd "$RELEASE_DIR"
sha256sum ./*.tar.xz ./*.tar.gz >SHASUMS256.txt
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS256.txt >SHASUMS256.asc
popd

pushd "$RELEASE_DIR/$RTX_VERSION"
sha256sum ./* >SHASUMS256.txt
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS256.txt >SHASUMS256.asc
popd

./rtx/scripts/render-install.sh >rtx.jdxcode.com/static/install.sh

rm -rf rtx.jdxcode.com/static/rpm
mv artifacts/rpm rtx.jdxcode.com/static/rpm

rm -rf rtx.jdxcode.com/static/deb
mv artifacts/deb rtx.jdxcode.com/static/deb

cp -vrf "$RELEASE_DIR/*" rtx.jdxcode.com/static

./rtx/scripts/release-npm.sh

pushd rtx.jdxcode.com
git add . && git commit -m "rtv $RTX_VERSION"
popd

./rtx/scripts/render-homebrew.sh >homebrew-tap/rtx.rb
pushd homebrew-tap
git add . && git commit -m "rtx $RTX_VERSION"
popd
