#!/bin/bash
# Container entry point: makes one machine, then records on it as `you`.
#
# usage: entry.sh MACHINE RUN CUTOFF VARIANT [UNTIL]
#
#   MACHINE  machine1 (C1 to C17) or machine2 (C18, C19)
#   VARIANT  systemd when machine 2 boots systemd (the history watcher runs
#            as a user service), plain when it has no service manager.
#            Machine 1 records the same either way, except that C16 declares
#            the watcher service only for systemd.
#
# Expects /rig (this directory) and /rig-bin/mise: the release binary the
# host downloaded and checked against its pinned sha256 (the container
# never downloads mise itself). Machine 2 also expects /srv/git/{api,setup}.git
# (pushed on machine 1) and /in/versions.json (resolved on machine 1).
# Writes /out/RUN.
set -euo pipefail
MACHINE=$1 RUN=$2 CUTOFF=$3 VARIANT=$4 UNTIL=${5:-}

test -x /rig-bin/mise || {
	echo "no /rig-bin/mise: the host copies the checked release binary in" >&2
	exit 1
}
install -m 0755 /rig-bin/mise /usr/local/bin/mise
got=$(/usr/local/bin/mise --version)
case "$got" in
*DEBUG*)
	echo "refusing a dev build: $got" >&2
	exit 1
	;;
esac
if [ -n "${CAPTURE_MISE_VERSION:-}" ] && [ "${got%% *}" != "$CAPTURE_MISE_VERSION" ]; then
	echo "expected mise $CAPTURE_MISE_VERSION, got $got" >&2
	exit 1
fi

# Placeholder repos: https://github.com/you/api and https://github.com/you/setup
# are local bare repos. This lives in /etc/gitconfig, never in ~/.gitconfig,
# so nothing in the home directory on screen mentions it. Longest match wins,
# so the .git spellings map too.
mkdir -p /srv/git
for r in api setup; do
	if [ "$MACHINE" = machine1 ]; then
		git init -q --bare -b main "/srv/git/$r.git"
	elif [ ! -d "/srv/git/$r.git" ]; then
		echo "machine 2 needs /srv/git/$r.git from machine 1" >&2
		exit 1
	fi
done
chown -R you:you /srv/git
cat >/etc/gitconfig <<'EOF'
[init]
	defaultBranch = main
[user]
	name = you
	email = you@example.com
[advice]
	detachedHead = false
[url "file:///srv/git/api.git"]
	insteadOf = https://github.com/you/api
	insteadOf = https://github.com/you/api.git
[url "file:///srv/git/setup.git"]
	insteadOf = https://github.com/you/setup
	insteadOf = https://github.com/you/setup.git
EOF

extra=()
if [ "$MACHINE" = machine2 ]; then
	test -f /in/versions.json || {
		echo "machine 2 needs /in/versions.json from machine 1" >&2
		exit 1
	}
	extra+=(--versions /in/versions.json)
	if [ "$VARIANT" = systemd ] && [ ! -S /run/user/1000/systemd/private ]; then
		echo "variant systemd, but the user manager for you is not running" >&2
		exit 1
	fi
fi

mkdir -p "/out/$RUN"
chown -R you:you /out
cd /home/you
exec runuser -u you -- env -i HOME=/home/you USER=you LOGNAME=you LANG=C.UTF-8 \
	PATH=/usr/local/bin:/usr/bin:/bin CAPTURE_IMAGE="${CAPTURE_IMAGE:-}" \
	python3 /rig/record.py --machine "$MACHINE" --variant "$VARIANT" \
	--steps /rig/steps.json --out "/out/$RUN" --cutoff "$CUTOFF" \
	"${extra[@]}" ${UNTIL:+--until "$UNTIL"}
