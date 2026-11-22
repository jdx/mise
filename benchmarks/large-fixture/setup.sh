#!/usr/bin/env bash
# Builds the work directory the large benchmarks in tak.toml run against:
#
#   setup.sh MISE_BIN WORK_DIR
#
# WORK_DIR gets a copy of this fixture as `project/` and its own HOME and mise
# data, config, state and cache directories, so nothing on the machine running
# the benchmark leaks into it. Every version the lockfile pins is installed as a
# fake: an install directory whose binaries are links to `true`. That is enough
# for `ls`, `env`, `hook-env` and `run` to do all of the work they do for a real
# install, without downloading anything.
#
# tak runs this once per benchmark; the benchmarks pass the same directories in
# their `env`, which is how they reach the state built here.
set -euo pipefail

mise=$(realpath "$1")
work=$2
fixture=$(cd "$(dirname "$0")" && pwd)

rm -rf "$work"
mkdir -p "$work/home"
work=$(realpath "$work")
cp -R "$fixture" "$work/project"
rm "$work/project/setup.sh"

# Only mise sees the fixture's directories: a shim on PATH, such as the `jq`
# below, would otherwise look for its tool in them. The ceiling keeps mise from
# reading configs in the directories above the work directory, such as a
# `~/.config/mise/config.toml` when the work directory is under HOME.
m() {
	HOME="$work/home" \
		MISE_DATA_DIR="$work/data" \
		MISE_CONFIG_DIR="$work/config" \
		MISE_STATE_DIR="$work/state" \
		MISE_CACHE_DIR="$work/cache" \
		MISE_CEILING_PATHS="$work" \
		MISE_OFFLINE=1 \
		MISE_YES=1 \
		"$mise" "$@"
}

cd "$work/project"
m trust --quiet --all

# A fake binary for every tool: `ls --json` reports where each locked version
# installs. Each gets a `bin/` holding a link named after the tool, which is
# where most backends' binaries live. The JSON is read with bash rather than
# jq: the benchmarks' HOME is also this script's, and a `jq` on PATH that is a
# mise shim would look for its tool under it.
true_bin=$(type -P true)
tool_re='^  "(.+)": \[$'
path_re='"install_path": "(.+)",?$'
m ls --json --current | while IFS= read -r line; do
	if [[ $line =~ $tool_re ]]; then
		name=${BASH_REMATCH[1]##*/}
		name=${name##*:}
	elif [[ $line =~ $path_re ]]; then
		mkdir -p "${BASH_REMATCH[1]}/bin"
		ln -sf "$true_bin" "${BASH_REMATCH[1]}/bin/$name"
	fi
done

# Prime the caches a second run would find, so the first sample is not the only
# one to build them.
m ls >/dev/null
(cd sub && m env >/dev/null)
