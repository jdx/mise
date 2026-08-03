#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
mise_bin=${MISE_BIN:-"$repo_root/target/release/mise"}
source_count=${TASK_CACHE_BENCH_SOURCES:-2000}
artifact_bytes=${TASK_CACHE_BENCH_ARTIFACT_BYTES:-33554432}
task_count=${TASK_CACHE_BENCH_TASKS:-500}
runs=${TASK_CACHE_BENCH_RUNS:-5}

for value in "$source_count" "$artifact_bytes" "$task_count" "$runs"; do
	if ! [[ $value =~ ^[1-9][0-9]*$ ]]; then
		echo "benchmark sizes and run count must be positive integers" >&2
		exit 2
	fi
done
if [[ ! -x $mise_bin ]]; then
	echo "mise benchmark binary is not executable: $mise_bin" >&2
	exit 2
fi

clock="date"
date_ns=$(date +%s%N)
if ! [[ $date_ns =~ ^[0-9]+$ ]]; then
	if command -v gdate >/dev/null && [[ $(gdate +%s%N) =~ ^[0-9]+$ ]]; then
		clock="gdate"
	elif command -v perl >/dev/null; then
		clock="perl"
	elif command -v python3 >/dev/null; then
		clock="python3"
	else
		echo "benchmark requires GNU date, perl, or python3 for high-resolution timing" >&2
		exit 2
	fi
fi

fixture=$(mktemp -d "${TMPDIR:-/tmp}/mise-task-cache-bench.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/sources" "$fixture/cache" "$fixture/state"

for ((i = 1; i <= source_count; i++)); do
	printf 'source-%08d\n' "$i" >"$fixture/sources/file-$i.txt"
done
dd if=/dev/urandom of="$fixture/sources/payload.bin" bs=1024 \
	count=$(((artifact_bytes + 1023) / 1024)) 2>/dev/null

{
	cat <<'EOF'
[settings]
experimental = true

[task_config.cache]
enabled = true

[tasks.hash]
run = "true"
sources = ["sources/**/*"]
outputs = []

[tasks.archive]
run = "rm -rf dist && mkdir dist && cp -R sources/. dist/"
sources = ["sources/**/*"]
outputs = ["dist"]

[tasks.graph]
run = "true"
outputs = []
depends = [
EOF
	for ((i = 1; i <= task_count; i++)); do
		printf '  "node-%d",\n' "$i"
	done
	cat <<'EOF'
]
EOF
	for ((i = 1; i <= task_count; i++)); do
		printf '\n[tasks."node-%d"]\nrun = "true"\noutputs = []\n' "$i"
	done
} >"$fixture/mise.toml"

export MISE_CACHE_DIR="$fixture/cache"
export MISE_STATE_DIR="$fixture/state"
export MISE_TRUSTED_CONFIG_PATHS="$fixture"
export MISE_TASK_CACHE_MAX_AGE=0
export MISE_TASK_CACHE_MAX_SIZE=0

now_ns() {
	case $clock in
	date) date +%s%N ;;
	gdate) gdate +%s%N ;;
	perl) perl -MTime::HiRes=time -e 'printf "%.0f\n", time() * 1_000_000_000' ;;
	python3) python3 -c 'import time; print(time.time_ns())' ;;
	esac
}

median() {
	sort -n | awk '{ samples[NR] = $1 } END {
    if (NR % 2) print samples[(NR + 1) / 2]
    else print int((samples[NR / 2] + samples[NR / 2 + 1]) / 2)
  }'
}

prepare_phase() {
	case $1 in
	archive-publish | restore) rm -rf "$fixture/dist" ;;
	esac
}

measure() {
	local phase=$1
	shift
	local samples=()
	for ((i = 1; i <= runs; i++)); do
		prepare_phase "$phase"
		local start end
		start=$(now_ns)
		"$@" >/dev/null 2>&1
		end=$(now_ns)
		samples+=("$(((end - start) / 1000000))")
	done
	printf '| %-15s | %9s | %4d |\n' "$phase" \
		"$(printf '%s\n' "${samples[@]}" | median) ms" "$runs"
}

# Populate every entry before timing cache-hit traversal and restoration.
"$mise_bin" -C "$fixture" run --force hash >/dev/null 2>&1
"$mise_bin" -C "$fixture" run --force archive >/dev/null 2>&1
"$mise_bin" -C "$fixture" run --jobs 1 graph >/dev/null 2>&1

printf 'task cache fixture: %d sources, %d artifact bytes, %d graph nodes\n\n' \
	"$source_count" "$artifact_bytes" "$task_count"
printf '| Phase           | Median    | Runs |\n'
printf '|-----------------|-----------|------|\n'
measure hashing "$mise_bin" -C "$fixture" run --force hash
measure archive-publish "$mise_bin" -C "$fixture" run --force archive
measure restore "$mise_bin" -C "$fixture" run archive
measure cached-graph "$mise_bin" -C "$fixture" run --jobs 1 graph

test -f "$fixture/dist/payload.bin"
