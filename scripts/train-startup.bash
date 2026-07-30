#!/usr/bin/env bash
# Exercise mise's startup-heavy paths for profile-guided optimizers.
#
# The caller is responsible for configuring its profiler. rustc PGO sets
# LLVM_PROFILE_FILE; BOLT instrumentation bakes its output path into the
# instrumented binary.
set -euo pipefail

if [ "$#" -ne 1 ]; then
	echo "usage: $0 <mise-binary>" >&2
	exit 1
fi

bin=$1
if [ ! -x "$bin" ]; then
	echo "ERROR: training binary is not executable: $bin" >&2
	exit 1
fi
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"

train_dir="$(mktemp -d "${TMPDIR:-/tmp}/mise-profile-train.XXXXXX")"
cleanup() {
	rm -rf "$train_dir"
}
trap cleanup EXIT

# Hermetic project fixture: env vars (including a template), pinned tools
# (never resolved against the network), and a trivial task.
mkdir -p "$train_dir/home" "$train_dir/proj/subdir"
cat >"$train_dir/proj/mise.toml" <<'EOF'
[env]
TRAIN_FOO = "bar"
TRAIN_TEMPLATED = "{{ env.HOME }}/x"

[tools]
node = "22.17.0"

[tasks.hello]
run = "true"
EOF
echo "node 22.17.0" >"$train_dir/proj/subdir/.tool-versions"

# Fresh isolated state per pass puts cold paths and warmed-up paths in the
# profile. LLVM_PROFILE_FILE may be empty for profilers other than rustc PGO.
# shellcheck disable=SC2016 # the single-quoted script takes $bin/$proj as args
train() {
	local pass=$1
	local root="$train_dir/state.$pass"
	mkdir -p "$root"
	env -i PATH="$PATH" HOME="$train_dir/home" TMPDIR="${TMPDIR:-/tmp}" \
		LLVM_PROFILE_FILE="${LLVM_PROFILE_FILE:-}" \
		MISE_DATA_DIR="$root/data" MISE_CACHE_DIR="$root/cache" \
		MISE_CONFIG_DIR="$root/config" MISE_STATE_DIR="$root/state" \
		MISE_GLOBAL_CONFIG_FILE="$root/config/config.toml" \
		MISE_OFFLINE=1 MISE_HIDE_UPDATE_WARNING=1 \
		bash -c '
			set -e
			bin=$1; proj=$2
			cd "$proj"
			"$bin" trust --all >/dev/null 2>&1 || true
			"$bin" version >/dev/null
			"$bin" current >/dev/null 2>&1 || true
			"$bin" ls >/dev/null 2>&1 || true
			"$bin" env -s zsh >/dev/null 2>&1 || true
			"$bin" env -s bash >/dev/null 2>&1 || true
			"$bin" settings >/dev/null 2>&1 || true
			"$bin" tasks ls >/dev/null 2>&1 || true
			"$bin" install >/dev/null 2>&1 || true
			"$bin" exec -- true >/dev/null 2>&1 || true
			# hook-env: full run, then evaluate the session so the
			# remaining runs take the per-prompt early-exit fast path.
			eval "$("$bin" hook-env -s bash 2>/dev/null)" || true
			for _ in 1 2 3 4 5; do
				"$bin" hook-env -s bash >/dev/null 2>&1 || true
			done
			# Subdirectory with .tool-versions: idiomatic-file parsing and
			# the config hierarchy walk.
			cd subdir
			"$bin" current >/dev/null 2>&1 || true
			"$bin" hook-env -s bash >/dev/null 2>&1 || true
		' -- "$bin" "$train_dir/proj"
}

for pass in 1 2 3; do
	echo "  train: pass $pass"
	train "$pass"
done
