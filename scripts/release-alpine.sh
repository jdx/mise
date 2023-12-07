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
git remote add jdxcode "https://jdxcode:$GITLAB_TOKEN@gitlab.alpinelinux.org/jdxcode/aports.git"
git checkout -mb rtx
cd community/rtx

sed -i "s/pkgver=.*/pkgver=${RTX_VERSION#v}/" APKBUILD

abuild checksum
cat /github/home/.abuild/abuild.conf
abuild -r
apkbuild-lint APKBUILD

git add APKBUILD
git checkout -b rtx
git commit -m "community/rtx: upgrade to ${RTX_VERSION#v}"

if [ "$DRY_RUN" == 0 ]; then
  git push jdxcode
fi

open_mr="$(gitlab mr list -R alpine/aports --author=@me)"
if [[ "$open_mr" != "Showing"* ]]; then
  if [ "$DRY_RUN" == 0 ]; then
    glab mr create --draft --fill --yes -H jdxcode/aports -R alpine/aports
  fi
fi
#git show
