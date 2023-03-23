#!/usr/bin/env bash
set -euxo pipefail

RTX_VERSION=$(./scripts/get-version.sh)

export GITLAB_HOST=gitlab.alpinelinux.org

sudo chown -R packager:packager /github/home
mkdir -p /github/home/.abuild
echo "$ALPINE_PUB_KEY" >/github/home/.abuild/-640e56d3.rsa.pub
echo "$ALPINE_PRIV_KEY" >/github/home/.abuild/-640e56d3.rsa
echo "PACKAGER_PRIVKEY=\"/github/home/.abuild/-640e56d3.rsa\"" >>/github/home/.abuild/abuild.conf

git config --global user.name "Jeff Dickey"
git config --global user.email 6271-jdxcode@users.gitlab.alpinelinux.org

git clone https://gitlab.alpinelinux.org/alpine/aports /home/packager/aports
cd /home/packager/aports
git config --local core.hooksPath .githooks
cd testing/rtx

sed -i "s/pkgver=.*/pkgver=${RTX_VERSION#v}/" APKBUILD
sed -i "s/cargo test --frozen/cargo test --all-features --frozen/" APKBUILD

abuild checksum
cat /github/home/.abuild/abuild.conf
abuild -r
apkbuild-lint APKBUILD

git add APKBUILD
git checkout -B "rtx/${RTX_VERSION#v}"
git commit -m "testing/rtx: upgrade to ${RTX_VERSION#v}"

git remote add jdxcode "https://jdxcode:$GITLAB_TOKEN@gitlab.alpinelinux.org/jdxcode/aports.git"
git push -f jdxcode
glab mr create --fill --yes -H jdxcode/aports -R alpine/aports
#git show
