#!/usr/bin/env bash
# Profile-Guided Optimization build for mise.
#
# Three-phase rustc PGO flow, adapted from jdx/aube's benchmarks/pgo.bash:
#
#   1. Build mise with -Cprofile-generate (instrumented binary).
#   2. Train against a hermetic, offline workload covering the startup
#      hot paths: settings/config load, toolset resolution, hook-env
#      (both the full run and the per-prompt early-exit fast path),
#      env rendering, task listing, and exec. No network, no registry —
#      everything runs in a throwaway HOME with isolated MISE_*_DIR s.
#   3. Merge .profraw via the rustup toolchain's llvm-profdata,
#      recompile with -Cprofile-use.
#
# The startup path dominates every mise invocation (shell prompts via
# hook-env, shims, version checks), so the training workload is biased
# toward short-lived invocations rather than long installs.
#
# Env hooks (mirroring aube's CI contract):
#   MISE_PGO_PROFILE=<profile>  cargo profile for both phases
#                               (default: serious, matching release
#                               tarballs).
#   MISE_PGO_TARGET=<triple>    cross-compilation target (default:
#                               host). The instrumented binary must be
#                               runnable on THIS machine for training —
#                               same-arch targets only.
#   MISE_PGO_BUILD_TOOL=<tool>  `cargo` (default) or `cross`. cross is
#                               used in CI for Linux targets so the
#                               binary keeps cross's older glibc
#                               baseline; the cross-built instrumented
#                               binary still runs on the host for
#                               training (older glibc is
#                               forward-compatible).
#   MISE_PGO_SKIP_FINAL_BUILD=1 stop after merging .profraw, for CI
#                               setups that delegate the final build to
#                               a separate step picking up RUSTFLAGS.
#
# Additional arguments are passed through to both cargo invocations —
# scripts/build-tarball.sh uses this for --no-default-features
# --features "...".
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

PGO_DATA_DIR="$REPO_ROOT/target/pgo-data"
PGO_PROFRAW_DIR="$PGO_DATA_DIR/profraw"
PGO_MERGED="$PGO_DATA_DIR/merged.profdata"

PGO_PROFILE="${MISE_PGO_PROFILE:-serious}"
PGO_TARGET="${MISE_PGO_TARGET:-}"
PGO_BUILD_TOOL="${MISE_PGO_BUILD_TOOL:-cargo}"

# target_arg stays unquoted at expansion sites: empty string disappears,
# "--target=foo" expands to one arg. Avoids bash 3.2 (macOS) array+set -u
# unbound-variable issues with "${arr[@]}".
target_arg=""
target_dir_part=""
if [ -n "$PGO_TARGET" ]; then
	target_arg="--target=$PGO_TARGET"
	target_dir_part="$PGO_TARGET/"
fi

RUSTC_HOST="$(rustc -vV | sed -n 's|^host: ||p')"
RUSTC_SYSROOT="$(rustc --print sysroot)"
LLVM_PROFDATA="$RUSTC_SYSROOT/lib/rustlib/$RUSTC_HOST/bin/llvm-profdata"
if [ ! -x "$LLVM_PROFDATA" ]; then
	echo "ERROR: llvm-profdata not found at $LLVM_PROFDATA" >&2
	echo "  Install with: rustup component add llvm-tools" >&2
	exit 1
fi

mkdir -p "$PGO_PROFRAW_DIR"
rm -f "$PGO_PROFRAW_DIR"/*.profraw "$PGO_MERGED"

# With MISE_PGO_BUILD_TOOL=cross, rustc runs inside a container that
# mounts the project at a container path, so when phase 3 reads
# -Cprofile-use=<host-path> from RUSTFLAGS the file is invisible.
# Bind-mount PGO_DATA_DIR at the same host path inside the container so
# the RUSTFLAGS value resolves. Harmless for phase 1.
if [ "$PGO_BUILD_TOOL" = "cross" ]; then
	export CROSS_CONTAINER_OPTS="${CROSS_CONTAINER_OPTS:-} -v $PGO_DATA_DIR:$PGO_DATA_DIR:rw"
fi

# ---------- Phase 1: instrumented build ----------
echo ">>> [1/3] Building instrumented binary ($PGO_BUILD_TOOL, profile=$PGO_PROFILE${PGO_TARGET:+, target=$PGO_TARGET})"
# shellcheck disable=SC2086 # intentional word-splitting on $target_arg
RUSTFLAGS="${RUSTFLAGS:-} -Cprofile-generate=$PGO_PROFRAW_DIR" \
	"$PGO_BUILD_TOOL" build --profile="$PGO_PROFILE" $target_arg --bin mise "$@"

INSTRUMENTED_BIN="$REPO_ROOT/target/${target_dir_part}${PGO_PROFILE}/mise"
if [ ! -x "$INSTRUMENTED_BIN" ]; then
	echo "ERROR: instrumented binary missing at $INSTRUMENTED_BIN" >&2
	exit 1
fi

# ---------- Phase 2: training ----------
echo ">>> [2/3] Training against hermetic offline workload"

train_dir="$(mktemp -d "${TMPDIR:-/tmp}/mise-pgo-train.XXXXXX")"
cleanup() {
	rm -rf "$train_dir"
}
trap cleanup EXIT

# Force the instrumented binary to write profraw to a path we control,
# regardless of what -Cprofile-generate=<dir> baked in at compile time
# (with cross, the compile-time path is a container path that doesn't
# resolve on the host). %m disambiguates per module; %p per process.
export LLVM_PROFILE_FILE="$PGO_PROFRAW_DIR/mise-%m-%p.profraw"

# Hermetic project fixture: env vars (incl. a template), pinned tools
# (never resolved against the network), and a trivial task. MISE_OFFLINE
# is belt-and-braces — nothing in the workload should hit the network.
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

# shellcheck disable=SC2016 # the single-quoted script takes $bin/$proj as args
train() {
	# Fresh isolated state per training pass so cold paths (state dirs,
	# trust, caches) and warm paths both land in the profile.
	local pass=$1
	local root="$train_dir/state.$pass"
	mkdir -p "$root"
	env -i PATH="$PATH" HOME="$train_dir/home" TMPDIR="${TMPDIR:-/tmp}" \
		LLVM_PROFILE_FILE="$LLVM_PROFILE_FILE" \
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
			# hook-env: full run, then eval the session so the
			# remaining runs take the per-prompt early-exit fast
			# path — the single hottest path in real usage.
			eval "$("$bin" hook-env -s bash 2>/dev/null)" || true
			for _ in 1 2 3 4 5; do
				"$bin" hook-env -s bash >/dev/null 2>&1 || true
			done
			# subdir with .tool-versions: idiomatic-file parsing +
			# config hierarchy walk
			cd subdir
			"$bin" current >/dev/null 2>&1 || true
			"$bin" hook-env -s bash >/dev/null 2>&1 || true
		' -- "$INSTRUMENTED_BIN" "$train_dir/proj"
}

for pass in 1 2 3; do
	echo "  train: pass $pass"
	train "$pass"
done
unset LLVM_PROFILE_FILE

# Sanity check: confirm training actually wrote profraw. On cross-built
# targets a bad LLVM_PROFILE_FILE path silently produces zero files and
# llvm-profdata merges nothing. Fail loudly here instead.
profraw_count=$(find "$PGO_PROFRAW_DIR" -maxdepth 1 -name '*.profraw' -type f | wc -l | tr -d ' ')
if [ "$profraw_count" -eq 0 ]; then
	echo "ERROR: no .profraw files written to $PGO_PROFRAW_DIR after training" >&2
	echo "  Training ran but the instrumented binary did not record profile data." >&2
	exit 1
fi
echo ">>> $profraw_count .profraw files collected"

# ---------- Phase 3a: merge ----------
echo ">>> [3/3] Merging profile data"
"$LLVM_PROFDATA" merge -o "$PGO_MERGED" "$PGO_PROFRAW_DIR"

# Defense in depth: a version mismatch between the rustc that
# instrumented and the llvm-profdata that merges can be a 0-exit silent
# no-op; phase 3b would then fail with an opaque missing-file error.
if [ ! -f "$PGO_MERGED" ]; then
	echo "ERROR: $PGO_MERGED was not produced by llvm-profdata merge" >&2
	exit 1
fi
echo ">>> merged profile written: $(stat -c %s "$PGO_MERGED" 2>/dev/null || stat -f %z "$PGO_MERGED") bytes"

if [ -n "${MISE_PGO_SKIP_FINAL_BUILD:-}" ]; then
	echo ">>> Skipping final optimized build (MISE_PGO_SKIP_FINAL_BUILD=1)"
	echo ">>> Profile ready at: $PGO_MERGED"
	exit 0
fi

# ---------- Phase 3b: optimize ----------
echo ">>> Rebuilding with -Cprofile-use"
# -Cllvm-args=-pgo-warn-missing-function=false: coverage gaps are
# expected (training can't exercise every path) and LLVM otherwise logs
# a note per uncovered symbol. Uncovered functions compile normally,
# just without PGO data.
# shellcheck disable=SC2086 # intentional word-splitting on $target_arg
RUSTFLAGS="${RUSTFLAGS:-} -Cprofile-use=$PGO_MERGED -Cllvm-args=-pgo-warn-missing-function=false" \
	"$PGO_BUILD_TOOL" build --profile="$PGO_PROFILE" $target_arg --bin mise "$@"

# Phase 3b wrote to the same path as phase 1, so the file at
# $INSTRUMENTED_BIN is now the PGO-optimized build, not the instrumented
# one. Alias for clarity.
PGO_FINAL_BIN="$INSTRUMENTED_BIN"
echo ">>> PGO build complete: $PGO_FINAL_BIN"
ls -lh "$PGO_FINAL_BIN"
