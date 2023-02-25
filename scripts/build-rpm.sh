#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

tar -xvJf "dist/rtx-rpm-$RTX_VERSION-linux-x64.tar.xz"
fpm -s dir -t rpm \
	--name rtx \
	--license MIT \
	--version "${RTX_VERSION#v*}" \
	--architecture x86_64 \
	--description "Polyglot runtime manager" \
	--url "https://github.com/jdxcode/rtx" \
	--maintainer "Jeff Dickey @jdxcode" \
	rtx/bin/rtx=/usr/bin/rtx \
	rtx/man/man1/rtx.1=/usr/share/man/man1/rtx.1

tar -xvJf "dist/rtx-rpm-$RTX_VERSION-linux-arm64.tar.xz"
fpm -s dir -t rpm \
	--name rtx \
	--license MIT \
	--version "${RTX_VERSION#v*}" \
	--architecture aarch64 \
	--description "Polyglot runtime manager" \
	--url "https://github.com/jdxcode/rtx" \
	--maintainer "Jeff Dickey @jdxcode" \
	rtx/bin/rtx=/usr/bin/rtx \
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
