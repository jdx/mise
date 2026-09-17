#!/usr/bin/env bash
# Unit tests for scripts/check-glibc.sh.
#
# The release builds run that check against real binaries, and those carry no
# C++ metadata -- so a regression that stopped rejecting libstdc++ would pass CI
# unnoticed. Drive the check with synthetic objdump output instead, so every
# rejection path has a test that fails when it stops rejecting.
set -euo pipefail

cd "$(dirname "$0")/.."
script=scripts/check-glibc.sh

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/bin"
binary="$tmp/fake-binary"
touch "$binary"

# Stand in for objdump: print the current fixture, or fail if it says FAIL.
cat >"$tmp/bin/objdump" <<'STUB'
#!/usr/bin/env bash
if [ "$(head -1 "$OBJDUMP_FIXTURE")" = "FAIL" ]; then
	echo "objdump: read error" >&2
	exit 1
fi
cat "$OBJDUMP_FIXTURE"
STUB
chmod +x "$tmp/bin/objdump"

PATH="$tmp/bin:$PATH"
export PATH
OBJDUMP_FIXTURE="$tmp/fixture"
export OBJDUMP_FIXTURE

failures=0
checks=0

# run_case <name> <expected-exit> [floor] <<< fixture on stdin
run_case() {
	local name=$1 want=$2 floor=${3:-2.26} out status
	cat >"$OBJDUMP_FIXTURE"
	checks=$((checks + 1))
	if out=$("$script" "$binary" "$floor" test-target 2>&1); then
		status=0
	else
		status=$?
	fi
	if [ "$status" -eq "$want" ]; then
		echo "ok       $name"
	else
		echo "FAIL     $name: expected exit $want, got $status"
		printf '%s\n' "$out" | sed 's/^/         /'
		failures=$((failures + 1))
	fi
}

run_case "clean binary passes" 0 <<'EOF'
  NEEDED               libc.so.6
  NEEDED               libgcc_s.so.1
  0x00 0x00 GLIBC_2.18
  0x00 0x00 GCC_4.2.0
EOF

run_case "libgcc_s GCC_* versions alone are not treated as C++" 0 <<'EOF'
  NEEDED               libgcc_s.so.1
  0x00 0x00 GLIBC_2.17
  0x00 0x00 GCC_3.0
  0x00 0x00 GCC_4.2.0
EOF

run_case "glibc newer than the floor is rejected" 1 <<'EOF'
  NEEDED               libc.so.6
  0x00 0x00 GLIBC_2.31
EOF

run_case "versioned C++ symbols are rejected" 1 <<'EOF'
  NEEDED               libc.so.6
  0x00 0x00 GLIBC_2.18
  0x00 0x00 GLIBCXX_3.4.29
  0x00 0x00 CXXABI_1.3.9
EOF

# The reason DT_NEEDED is checked separately: --no-as-needed, or a transitive
# pickup, links libstdc++ without importing any versioned C++ symbol.
run_case "libstdc++ in NEEDED is rejected without versioned symbols" 1 <<'EOF'
  NEEDED               libstdc++.so.6
  NEEDED               libc.so.6
  0x00 0x00 GLIBC_2.18
EOF

run_case "libstdc++ is rejected even with no GLIBC_ symbols at all" 1 <<'EOF'
  NEEDED               libstdc++.so.6
EOF

run_case "a binary with no GLIBC_ symbols and no C++ passes" 0 <<'EOF'
  NEEDED               libc.so.6
EOF

run_case "a similarly-named library is not mistaken for libstdc++" 0 <<'EOF'
  NEEDED               libstdbuf.so
  0x00 0x00 GLIBC_2.18
EOF

run_case "a failing objdump aborts instead of reporting success" 1 <<'EOF'
FAIL
EOF

# Not a fixture case: the binary itself is missing.
checks=$((checks + 1))
if "$script" "$tmp/does-not-exist" 2.26 test-target >/dev/null 2>&1; then
	echo "FAIL     a missing binary is rejected: expected exit 1, got 0"
	failures=$((failures + 1))
else
	echo "ok       a missing binary is rejected"
fi

echo
if [ "$failures" -ne 0 ]; then
	echo "$failures of $checks check-glibc.sh tests failed"
	exit 1
fi
echo "all $checks check-glibc.sh tests passed"
