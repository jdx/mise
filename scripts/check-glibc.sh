#!/usr/bin/env bash
set -euo pipefail

binary_path=${1:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
max_allowed=${2:?"usage: check-glibc.sh <binary> <max-glibc> <target-name>"}
target_name=${3:-$binary_path}

if [[ ! -f $binary_path ]]; then
	echo "Error: binary not found at $binary_path; aborting glibc check" >&2
	exit 1
fi

# Read the dynamic section once, and fail loudly if that read fails. Both checks
# below work from this one snapshot, so a broken objdump cannot leave either of
# them quietly concluding that it found nothing.
if ! dynamic_info=$(objdump -p "$binary_path"); then
	echo "Error: objdump could not read $binary_path; aborting glibc check" >&2
	exit 1
fi

max_required=$(
	awk '/GLIBC_/ { sub(/.*GLIBC_/, ""); print }' <<<"$dynamic_info" |
		sort -V |
		tail -1
)

if [[ -z $max_required ]]; then
	echo "No glibc symbols found for $target_name"
else
	echo "Maximum glibc version required for $target_name: $max_required"

	if printf '%s\n' "$max_required" "$max_allowed" | sort -V -C; then
		echo "Binary is compatible with $target_name (glibc $max_required <= $max_allowed)"
	else
		echo "Binary requires glibc $max_required, which is newer than $target_name's glibc $max_allowed"
		echo "This binary will NOT work on $target_name"
		exit 1
	fi
fi

# The check above only sees glibc. C++ symbol versions live in libstdc++ and are
# invisible to it: "GLIBCXX_3.4.29" does not contain the substring "GLIBC_", so
# the /GLIBC_/ match skips it. A C++ dependency would otherwise add a second,
# unreported compatibility floor while this script still reported success.
#
# DT_NEEDED is the authoritative signal. A binary can link libstdc++ without
# importing any versioned C++ symbol -- linked with --no-as-needed, or picked up
# transitively -- and the version list would then stay empty while the
# dependency is real. Test the dependency itself; keep the versions as
# diagnostics for whoever has to act on the failure.
#
# mise links no C++ today: the released binaries list no libstdc++ in NEEDED and
# carry no GLIBCXX_/CXXABI_ versions. Treat any appearance as a regression
# rather than guessing a threshold for it. libgcc_s's GCC_* versions are
# expected on a Rust binary and are deliberately not matched.
needed_libstdcxx=$(awk '$1 == "NEEDED" && $2 ~ /^libstdc\+\+\./ { print $2 }' <<<"$dynamic_info")

# Only grep's "no matches" exit status is expected, so scope the tolerance to
# grep alone -- a genuine sort failure must still fail the build.
cxx_versions=$(
	{ grep -oE '(GLIBCXX|CXXABI)_[0-9.]+' <<<"$dynamic_info" || true; } |
		sort -u -V |
		sed 's/^/  /'
)

if [[ -n $needed_libstdcxx || -n $cxx_versions ]]; then
	echo "Binary depends on libstdc++, which adds a compatibility floor this check cannot verify" >&2
	if [[ -n $needed_libstdcxx ]]; then
		echo "Dynamic dependency: $needed_libstdcxx" >&2
	fi
	if [[ -n $cxx_versions ]]; then
		echo "Required C++ symbol versions:" >&2
		echo "$cxx_versions" >&2
	fi
	echo "Link C++ code statically (-static-libstdc++ -static-libgcc), or extend this script with a libstdc++ threshold for $target_name" >&2
	exit 1
fi

echo "No libstdc++ dependency for $target_name"
