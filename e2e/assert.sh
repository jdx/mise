# shellcheck shell=bash

# shellcheck source-path=SCRIPTDIR
source "$MISE_TEST_ROOT/style.sh"

fail() {
  err "$*"
  exit 1
}

# Safeguard against running test directly, which would execute in the actual user home
[[ -n "${MISE_TEST_NAME:-}" ]] || fail "tests should be called using run_test"

quiet_assert_succeed() {
  local status=0
  bash -c """$1""" || status=$?
  if [[ $status -ne 0 ]]; then
    fail "[""$1""] command failed with status "$status""
  fi
}

assert_succeed() {
  if quiet_assert_succeed """$1"""; then
    ok "[""$1""] expected success"
  fi
}

assert_fail() {
  if ! bash -c """$1""" 2>&1; then
    ok "[""$1""] expected failure"
  else
    fail "[""$1""] expected failure but succeeded"
  fi
}

assert() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ ""$actual"" == ""$2"" ]]; then
    ok "[""$1""] output is equal to '$2'"
  else
    fail "[""$1""] expected '$2' but got '$actual'"
  fi
}

assert_not() {
  local actual
  actual="$(bash -c "$1" || true)"
  if [[ ""$actual"" != ""$2"" ]]; then
    ok "[""$1""] output is different from '$2'"
  else
    fail "[""$1""] expected '$2' not to be in '$actual'"
  fi
}

assert_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ ""$actual"" == *""$2""* ]]; then
    ok "[""$1""] '$2' is in output"
  else
    fail "[""$1""] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ ""$actual"" != *""$2""* ]]; then
    ok "[""$1""] '$2' is not in output"
  else
    fail "[""$1""] expected '$2' not to be in '$actual'"
  fi
}

assert_matches() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ ""$actual"" =~ $2 ]]; then
    ok "[""$1""] '$2' matches output"
  else
    fail "[""$1""] expected '$2' to match '$actual'"
  fi
}

skip_slow_test() {
  if [[ -z ""${TEST_ALL:-}"" ]]; then
    warn "skipping slow tests"
    exit 0
  fi
}

require_cmd() {
  if ! type -p """$1""" >/dev/null; then
    warn "skipping test: cannot find ""$1"" in PATH ($PATH)"
    exit 0
  fi
}
