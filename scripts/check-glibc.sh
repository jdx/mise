#!/usr/bin/env bash
set -euo pipefail

binary_path=${1:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
max_allowed=${2:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
target_name=${3:-$binary_path}

if [[ ! -f $binary_path ]]; then
	echo "Error: binary not found at $binary_path; aborting glibc check" >&2
	exit 1
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

# The check above only sees glibc. C++ symbol versions live in libstdc++ and are
# invisible to it: "GLIBCXX_3.4.29" does not contain the substring "GLIBC_", so
# the /GLIBC_/ match skips it. A C++ dependency would therefore add a second,
# unreported compatibility floor while this script still reported success.
#
# mise links no C++ today (libstdc++ is absent from NEEDED, and the released
# binary carries no GLIBCXX_/CXXABI_ versions), so treat any appearance as a
# regression rather than guessing a threshold for it. libgcc_s's GCC_* versions
# are expected on a Rust binary and are deliberately not matched here.
cxx_versions=$(
	objdump -p "$binary_path" |
		grep -oE '(GLIBCXX|CXXABI)_[0-9.]+' |
		sort -u -V |
		sed 's/^/  /' || true
)

if [[ -n $cxx_versions ]]; then
	echo "Binary depends on libstdc++, which adds a compatibility floor this check cannot verify" >&2
	echo "Required C++ symbol versions:" >&2
	echo "$cxx_versions" >&2
	echo "Link C++ code statically (-static-libstdc++ -static-libgcc), or extend this script with a libstdc++ threshold for $target_name" >&2
	exit 1
fi

echo "No libstdc++ dependency for $target_name"
