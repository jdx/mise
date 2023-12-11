#!/usr/bin/env bash
set -euxo pipefail

git config --global user.name rtx-vm
git config --global user.email 123107610+rtx-vm@users.noreply.github.com

RTX_VERSION=$(cd rtx && ./scripts/get-version.sh)
RELEASE_DIR=releases
export RTX_VERSION RELEASE_DIR
rm -rf "${RELEASE_DIR:?}/$RTX_VERSION"
mkdir -p "$RELEASE_DIR/$RTX_VERSION"

find artifacts -name 'tarball-*' -exec sh -c '
  target=${1#artifacts/tarball-}
  cp "artifacts/tarball-$target/"*.tar.gz "$RELEASE_DIR/$RTX_VERSION"
  cp "artifacts/tarball-$target/"*.tar.xz "$RELEASE_DIR/$RTX_VERSION"
  ' sh {} \;

platforms=(
  linux-x64
  linux-x64-musl
  linux-arm64
  linux-arm64-musl
  linux-armv6
  linux-armv6-musl
  linux-armv7
  linux-armv7-musl
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
echo "$RTX_VERSION" | tr -d 'v' >VERSION
cp rtx-latest-linux-x64 rtx-latest-linux-amd64
cp rtx-latest-macos-x64 rtx-latest-macos-amd64
sha256sum ./rtx-latest-* >SHASUMS256.txt
sha512sum ./rtx-latest-* >SHASUMS512.txt
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS512.txt >SHASUMS512.asc
popd

pushd "$RELEASE_DIR/$RTX_VERSION"
sha256sum ./* >SHASUMS256.txt
sha512sum ./* >SHASUMS512.txt
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 408B88DB29DDE9E0 <SHASUMS512.txt >SHASUMS512.asc
popd

./rtx/scripts/render-install.sh >"$RELEASE_DIR"/install.sh
chmod +x "$RELEASE_DIR"/install.sh
shellcheck "$RELEASE_DIR"/install.sh
# TODO: figure out how to test this
# "$RELEASE_DIR"/install.sh
#~/.local/share/rtx/bin/rtx -v
gpg -u 408B88DB29DDE9E0 --output "$RELEASE_DIR"/install.sh.sig --sign "$RELEASE_DIR"/install.sh

if [[ "$DRY_RUN" != 1 ]]; then
  NPM_PREFIX=@jdxcode/rtx ./rtx/scripts/release-npm.sh
  NPM_PREFIX=rtx-cli ./rtx/scripts/release-npm.sh
  #AWS_S3_BUCKET=rtx.jdx.dev ./rtx/scripts/publish-s3.sh
  ./rtx/scripts/publish-r2.sh
fi

./rtx/scripts/render-homebrew.sh >homebrew-tap/rtx.rb
pushd homebrew-tap
git add . && git commit -m "rtx ${RTX_VERSION#v}"
popd
