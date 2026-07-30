#!/usr/bin/env bash
# Apply profile-guided LLVM BOLT optimizations to a linked mise ELF binary.
set -euo pipefail

if [ "$#" -ne 1 ]; then
	echo "usage: $0 <mise-binary>" >&2
	exit 1
fi

binary=$1
if [ ! -x "$binary" ]; then
	echo "ERROR: BOLT input is not executable: $binary" >&2
	exit 1
fi
binary="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"

llvm_bolt="${LLVM_BOLT:-llvm-bolt}"
merge_fdata="${MERGE_FDATA:-merge-fdata}"
for tool in "$llvm_bolt" "$merge_fdata"; do
	if ! command -v "$tool" >/dev/null 2>&1; then
		echo "ERROR: required BOLT tool not found: $tool" >&2
		exit 1
	fi
done

llvm_bolt_path="$(readlink -f "$(command -v "$llvm_bolt")")"
llvm_bolt_prefix="$(cd "$(dirname "$llvm_bolt_path")/.." && pwd)"
runtime_instrumentation_lib="${BOLT_RUNTIME_INSTRUMENTATION_LIB:-$llvm_bolt_prefix/lib/libbolt_rt_instr.a}"
if [ ! -f "$runtime_instrumentation_lib" ]; then
	echo "ERROR: BOLT instrumentation runtime not found: $runtime_instrumentation_lib" >&2
	echo "  Set BOLT_RUNTIME_INSTRUMENTATION_LIB to libbolt_rt_instr.a." >&2
	exit 1
fi
runtime_instrumentation_arg="$runtime_instrumentation_lib"
case "$runtime_instrumentation_arg" in
/usr/lib/*)
	# Debian's BOLT resolves this option relative to /usr/lib even when an
	# absolute path is supplied, which would otherwise produce /usr/lib/usr/lib.
	runtime_instrumentation_arg="${runtime_instrumentation_arg#/usr/lib/}"
	;;
esac

if ! readelf -S "$binary" | grep -q '\.rela\.text'; then
	echo "ERROR: $binary has no .rela.text section" >&2
	echo "  Link it with -Wl,--emit-relocs before running BOLT." >&2
	exit 1
fi

bolt_dir="$(mktemp -d "${TMPDIR:-/tmp}/mise-bolt.XXXXXX")"
instrumented="$bolt_dir/mise.instrumented"
profile_prefix="$bolt_dir/mise.fdata"
merged_profile="$bolt_dir/merged.fdata"
optimized="$binary.bolt"
cleanup() {
	rm -rf "$bolt_dir"
	rm -f "$optimized"
}
trap cleanup EXIT

echo ">>> [1/3] Instrumenting PGO-optimized binary with BOLT"
"$llvm_bolt" "$binary" \
	-instrument \
	-runtime-instrumentation-lib="$runtime_instrumentation_arg" \
	-instrumentation-file="$profile_prefix" \
	-instrumentation-file-append-pid \
	-o "$instrumented"

echo ">>> [2/3] Training BOLT against hermetic offline workload"
bash "$(dirname "$0")/train-startup.bash" "$instrumented"

profile_count=$(find "$bolt_dir" -maxdepth 1 -name 'mise.fdata.*' -type f | wc -l | tr -d ' ')
if [ "$profile_count" -eq 0 ]; then
	echo "ERROR: BOLT training produced no profiles in $bolt_dir" >&2
	exit 1
fi
echo ">>> $profile_count BOLT profiles collected"
# BOLT appends only numeric process IDs to this controlled prefix, so the glob
# cannot contain whitespace and is safe to expand here.
# shellcheck disable=SC2086
"$merge_fdata" "$profile_prefix".* >"$merged_profile"

echo ">>> [3/3] Reordering and splitting hot code with BOLT"
"$llvm_bolt" "$binary" \
	-o "$optimized" \
	-data="$merged_profile" \
	-reorder-blocks=ext-tsp \
	-reorder-functions=cdsort \
	-split-functions \
	-split-all-cold \
	-split-eh \
	-dyno-stats

"$optimized" version >/dev/null
mv "$optimized" "$binary"
echo ">>> BOLT optimization complete: $binary"
ls -lh "$binary"
