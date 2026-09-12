#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-latest-version.sh)

export GITLAB_HOST=gitlab.alpinelinux.org
export GITLAB_TOKEN="$ALPINE_GITLAB_TOKEN"

sudo chown -R packager:packager /github/home
mkdir -p /github/home/.abuild
echo "$ALPINE_PUB_KEY" | sudo tee "/etc/apk/keys/$ALPINE_KEY_ID.pub"
echo "$ALPINE_PUB_KEY" >"/github/home/.abuild/$ALPINE_KEY_ID.pub"
echo "$ALPINE_PRIV_KEY" >"/github/home/.abuild/$ALPINE_KEY_ID"
echo "PACKAGER_PRIVKEY=\"/github/home/.abuild/$ALPINE_KEY_ID\"" >>/github/home/.abuild/abuild.conf

git config --global user.name "Jeff Dickey"
git config --global user.email 6271-jdxcode@users.gitlab.alpinelinux.org

git clone https://gitlab.alpinelinux.org/alpine/aports.git/ /home/packager/aports
cd /home/packager/aports
git config --local core.hooksPath .githooks
git remote add jdxcode "https://jdxcode:$GITLAB_TOKEN@gitlab.alpinelinux.org/jdxcode/aports.git/"
git checkout -mb mise
cd community/mise

sed -i "s/pkgver=.*/pkgver=${MISE_VERSION#v}/" APKBUILD

abuild checksum
cat /github/home/.abuild/abuild.conf
abuild -r
#apkbuild-lint APKBUILD fails due to: SC:[AL57]:./APKBUILD:7:invalid arch '!loongarch64'

git add APKBUILD

if git diff --cached --exit-code; then
	echo "No changes to commit"
	exit 0
fi
git commit -m "community/mise: upgrade to ${MISE_VERSION#v}"

if [ "$DRY_RUN" == 0 ]; then
	git push jdxcode -f
fi

open_mr="$(glab mr list -R alpine/aports --author=@me)"
if [[ $open_mr != "Showing"* ]]; then
	if [ "$DRY_RUN" == 0 ]; then
		DEBUG=1 glab mr create --fill --yes -H jdxcode/aports -R alpine/aports
	fi
fi
#git show
