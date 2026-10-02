#!/usr/bin/env bash
set -euxo pipefail

MISE_VERSION=$(./scripts/get-latest-version.sh)

export GITLAB_HOST=gitlab.alpinelinux.org
export GITLAB_TOKEN="$ALPINE_GITLAB_TOKEN"
# GitHub repo (a fork of alpinelinux/aports) that receives a copy of each bump
ALPINE_BACKUP_REPO="${ALPINE_BACKUP_REPO:-jdx/aports}"

sudo chown -R packager:packager /github/home
mkdir -p /github/home/.abuild
echo "$ALPINE_PUB_KEY" | sudo tee "/etc/apk/keys/$ALPINE_KEY_ID.pub"
echo "$ALPINE_PUB_KEY" >"/github/home/.abuild/$ALPINE_KEY_ID.pub"
echo "$ALPINE_PRIV_KEY" >"/github/home/.abuild/$ALPINE_KEY_ID"
echo "PACKAGER_PRIVKEY=\"/github/home/.abuild/$ALPINE_KEY_ID\"" >>/github/home/.abuild/abuild.conf

git config --global user.name "Jeff Dickey"
git config --global user.email 6271-jdxcode@users.gitlab.alpinelinux.org

# gitlab.alpinelinux.org answers git requests from CI IP ranges with an HTTP 418
# anti-bot challenge, with or without credentials, so clone the GitHub mirror
# instead. It tracks the same history; we still push and open the MR on GitLab.
git clone https://github.com/alpinelinux/aports.git /home/packager/aports
cd /home/packager/aports
git config --local core.hooksPath .githooks
if [ -n "${ALPINE_GITLAB_SSH_KEY:-}" ]; then
	# The 418 challenge is served by the HTTP proxy in front of GitLab; sshd is
	# not behind it, so pushing over SSH avoids it. Optional until the deploy key
	# secret exists, then preferred over HTTPS.
	sudo apk add --no-cache openssh-client
	mkdir -p /home/packager/.ssh
	chmod 700 /home/packager/.ssh
	# keep the key out of the xtrace output
	{ set +x; } 2>/dev/null
	echo "$ALPINE_GITLAB_SSH_KEY" >/home/packager/.ssh/gitlab_alpine
	set -x
	chmod 600 /home/packager/.ssh/gitlab_alpine
	echo "gitlab.alpinelinux.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA3AxnPM+Cquq/2QXWWmuHdgmfMIU+v/7kV/k3p+KpBd" >/home/packager/.ssh/known_hosts
	export GIT_SSH_COMMAND="ssh -i /home/packager/.ssh/gitlab_alpine -o IdentitiesOnly=yes -o UserKnownHostsFile=/home/packager/.ssh/known_hosts -o StrictHostKeyChecking=yes"
	git remote add jdxcode "git@gitlab.alpinelinux.org:jdxcode/aports.git"
else
	git remote add jdxcode "https://jdxcode:$GITLAB_TOKEN@gitlab.alpinelinux.org/jdxcode/aports.git/"
fi
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
	# Keep a copy of the bump on GitHub first. gitlab.alpinelinux.org may answer
	# the push below with an HTTP 418 anti-bot challenge from CI IP ranges; the
	# build takes ~40 minutes, so this lets someone finish the push from a
	# machine GitLab accepts instead of re-running the whole job. Best effort:
	# a failure here must not block the real push.
	{ set +x; } 2>/dev/null
	git remote add backup "https://x-access-token:$GH_TOKEN@github.com/$ALPINE_BACKUP_REPO.git"
	set -x
	backup_branch="mise-${MISE_VERSION#v}"
	backup_ok=0
	if git push backup "HEAD:refs/heads/$backup_branch" -f; then
		backup_ok=1
	else
		echo "::warning::could not push $backup_branch to github.com/$ALPINE_BACKUP_REPO"
	fi

	pushed=0
	for attempt in 1 2 3; do
		if git push jdxcode -f; then
			pushed=1
			break
		fi
		echo "git push to gitlab.alpinelinux.org failed (attempt $attempt/3)"
		if [ "$attempt" -lt 3 ]; then
			sleep $((attempt * 60))
		fi
	done
	if [ "$pushed" == 0 ]; then
		echo "::error::could not push to gitlab.alpinelinux.org/jdxcode/aports (likely an HTTP 418 bot challenge on CI IPs)."
		if [ "$backup_ok" == 1 ]; then
			cat <<EOF
The bump is committed at github.com/$ALPINE_BACKUP_REPO branch $backup_branch. To finish by hand:
  git clone --branch $backup_branch --single-branch https://github.com/$ALPINE_BACKUP_REPO.git aports
  cd aports && git checkout -b mise
  git push git@gitlab.alpinelinux.org:jdxcode/aports.git mise -f
  GITLAB_HOST=gitlab.alpinelinux.org glab mr create --fill --yes -H jdxcode/aports -R alpine/aports
EOF
		else
			cat <<EOF
The GitHub backup push also failed, so the commit only existed in this job. Re-run the job, or
bump community/mise/APKBUILD (pkgver, then abuild checksum) by hand and push it to jdxcode/aports.
EOF
		fi
		exit 1
	fi
fi

open_mr="$(glab mr list -R alpine/aports --author=@me)"
if [[ $open_mr != "Showing"* ]]; then
	if [ "$DRY_RUN" == 0 ]; then
		DEBUG=1 glab mr create --fill --yes -H jdxcode/aports -R alpine/aports
	fi
fi
#git show
