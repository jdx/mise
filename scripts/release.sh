#!/usr/bin/env bash
set -euxo pipefail

echo "::group::Setup"
git config --global user.name mise-en-dev
git config --global user.email 123107610+mise-en-dev@users.noreply.github.com

BASE_DIR="$(pwd)"
MISE_VERSION=$(./scripts/get-version.sh)
RELEASE_DIR=releases
export BASE_DIR MISE_VERSION RELEASE_DIR
rm -rf "${RELEASE_DIR:?}/$MISE_VERSION"
mkdir -p "$RELEASE_DIR/$MISE_VERSION"

echo "::group::Build"
platforms=(
	linux-x64
	linux-x64-musl
	linux-arm64
	linux-arm64-musl
	linux-armv7
	linux-armv7-musl
	macos-x64
	macos-arm64
)
for tls_suffix in "" "-nativetls"; do
	for platform in "${platforms[@]}"; do
		cp artifacts/*/"mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz" "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz"
		cp artifacts/*/"mise-$MISE_VERSION-${platform}${tls_suffix}.tar.xz" "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.xz"
		cp artifacts/*/"mise-$MISE_VERSION-${platform}${tls_suffix}.tar.zst" "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.zst"
		zipsign sign tar "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz" ~/.zipsign/mise.priv
		zipsign verify tar "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz" "$BASE_DIR/zipsign.pub"
		cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz" "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}.tar.gz"
		cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.xz" "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}.tar.xz"
		cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.zst" "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}.tar.zst"
		tar -xvzf "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.tar.gz"
		cp -v mise/bin/mise "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}"
		cp -v mise/bin/mise "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}"
	done
done

windows_platforms=(
	windows-arm64
	windows-x64
)
for tls_suffix in "" "-nativetls"; do
	for platform in "${windows_platforms[@]}"; do
		cp artifacts/*/"mise-$MISE_VERSION-${platform}${tls_suffix}.zip" "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.zip"
		zipsign sign zip "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.zip" ~/.zipsign/mise.priv
		zipsign verify zip "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.zip" "$BASE_DIR/zipsign.pub"
		cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.zip" "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}.zip"
		unzip -p "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.zip" mise/bin/mise.exe >"$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.exe"
		cp "$RELEASE_DIR/$MISE_VERSION/mise-$MISE_VERSION-${platform}${tls_suffix}.exe" "$RELEASE_DIR/mise-latest-${platform}${tls_suffix}.exe"
	done
done

echo "::group::Checksums"
pushd "$RELEASE_DIR"
cp mise-latest-linux-x64 mise-latest-linux-amd64
cp mise-latest-macos-x64 mise-latest-macos-amd64
cp mise-latest-linux-x64-nativetls mise-latest-linux-amd64-nativetls
cp mise-latest-macos-x64-nativetls mise-latest-macos-amd64-nativetls
sha256sum ./mise-latest-* >SHASUMS256.txt
sha512sum ./mise-latest-* >SHASUMS512.txt
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS512.txt >SHASUMS512.asc
minisign -WSs "$BASE_DIR/minisign.key" -p "$BASE_DIR/minisign.pub" -m SHASUMS256.txt SHASUMS512.txt </dev/zero
popd

pushd "$RELEASE_DIR/$MISE_VERSION"
sha256sum ./* >SHASUMS256.txt
sha512sum ./* >SHASUMS512.txt
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS256.txt >SHASUMS256.asc
gpg --clearsign -u 8B81C9D17413A06D <SHASUMS512.txt >SHASUMS512.asc
minisign -WSs "$BASE_DIR/minisign.key" -p "$BASE_DIR/minisign.pub" -m SHASUMS256.txt SHASUMS512.txt </dev/zero
popd

echo "::group::install.sh"
./scripts/render-install.sh >"$RELEASE_DIR"/install.sh
chmod +x "$RELEASE_DIR"/install.sh
shellcheck "$RELEASE_DIR"/install.sh
gpg -u 8B81C9D17413A06D --output "$RELEASE_DIR"/install.sh.sig --sign "$RELEASE_DIR"/install.sh
minisign -WSs "$BASE_DIR/minisign.key" -p "$BASE_DIR/minisign.pub" -m "$RELEASE_DIR"/install.sh </dev/zero
cp "$RELEASE_DIR"/{install.sh,install.sh.sig,install.sh.minisig} "$RELEASE_DIR/$MISE_VERSION"

echo "::group::mise.run scripts"
./scripts/render-mise-run.sh

echo "::group::Sign source tarball"
TMP_FILE="$(mktemp)"
curl -L -o "$TMP_FILE" "https://github.com/jdx/mise/archive/refs/tags/$MISE_VERSION.tar.gz"
gpg --detach-sign -u 8B81C9D17413A06D <"$TMP_FILE" >"$RELEASE_DIR/$MISE_VERSION/$MISE_VERSION.tar.gz.sig"
rm "$TMP_FILE"

# Publishing is now done after GitHub release is created
