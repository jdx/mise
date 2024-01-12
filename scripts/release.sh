#!/usr/bin/env bash
set -euxo pipefail

echo "::group::Setup"
git config --global user.name mise-en-dev
git config --global user.email 123107610+mise-en-dev@users.noreply.github.com

BASE_DIR="$(cd mise && pwd)"
MISE_VERSION=$(cd mise && ./scripts/get-version.sh)
RELEASE_DIR=releases
export BASE_DIR MISE_VERSION RELEASE_DIR
rm -rf "${RELEASE_DIR:?}/$MISE_VERSION"
mkdir -p "$RELEASE_DIR/$MISE_VERSION"

echo "::group::Build"
find artifacts -name 'tarball-*' -exec sh -c '
  target=${1#artifacts/tarball-}
  cp "artifacts/tarball-$target/"*.tar.gz "$RELEASE_DIR/$MISE_VERSION"
  cp "artifacts/tarball-$target/"*.tar.xz "$RELEASE_DIR/$MISE_VERSION"
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
  cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform.tar.gz" "$RELEASE_DIR/mise-latest-$platform.tar.gz"
  cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform.tar.xz" "$RELEASE_DIR/mise-latest-$platform.tar.xz"
  tar -xvzf "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform.tar.gz"
  cp -v mise/bin/mise "$RELEASE_DIR/mise-latest-$platform"
  cp -v mise/bin/mise "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-$platform"
done

echo "::group::Checksums"
pushd "$RELEASE_DIR"
echo "$MISE_VERSION" | tr -d 'v' >VERSION
cp mise-latest-linux-x64 mise-latest-linux-amd64
cp mise-latest-macos-x64 mise-latest-macos-amd64
sha256sum ./mise-latest-* >SHASUMS256.txt
sha512sum ./mise-latest-* >SHASUMS512.txt
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS512.txt >SHASUMS512.asc
popd

pushd "$RELEASE_DIR/$MISE_VERSION"
sha256sum ./* >SHASUMS256.txt
sha512sum ./* >SHASUMS512.txt
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS512.txt >SHASUMS512.asc
popd

echo "::group::install.sh"
./mise/scripts/render-install.sh >"$RELEASE_DIR"/install.sh
chmod +x "$RELEASE_DIR"/install.sh
shellcheck "$RELEASE_DIR"/install.sh
gpg -u 8B81C9D17413A06D --output "$RELEASE_DIR"/install.sh.sig --sign "$RELEASE_DIR"/install.sh

if [[ "$DRY_RUN" != 1 ]]; then
  echo "::group::Publish npm @jdxcode/mise"
  NPM_PREFIX=@jdxcode/mise ./mise/scripts/release-npm.sh
  #  echo "::group::Publish npm mise-cli"
  #  NPM_PREFIX=mise-cli ./mise/scripts/release-npm.sh
  echo "::group::Publish r2"
  ./mise/scripts/publish-r2.sh
fi

echo "::group::Publish mise-docs"
cp ./mise/docs/cli-reference.md ./mise-docs/cli/index.md
pushd mise-docs
if [[ -z $(git status -s) ]]; then
  echo "No changes to docs"
else
  git add cli/index.md
  git commit -m "mise ${MISE_VERSION#v}"
fi
popd
