#!/usr/bin/env bash
set -euo pipefail

binary_path=${1:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
max_allowed=${2:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
target_name=${3:-$binary_path}

if [[ ! -f $binary_path ]]; then
	echo "Warning: binary not found at $binary_path, skipping glibc check"
	exit 0
fi

max_required=$(
	objdump -p "$binary_path" |
		awk '/GLIBC_/ { sub(/.*GLIBC_/, ""); print }' |
		sort -V |
		tail -1
)

if [[ -z $max_required ]]; then
	echo "No glibc symbols found for $target_name"
	exit 0
fi

echo "Maximum glibc version required for $target_name: $max_required"

if printf '%s\n' "$max_required" "$max_allowed" | sort -V -C; then
	echo "Binary is compatible with $target_name (glibc $max_required <= $max_allowed)"
else
	echo "Binary requires glibc $max_required, which is newer than $target_name's glibc $max_allowed"
	echo "This binary will NOT work on $target_name"
	exit 1
fi
