# shellcheck shell=bash

# shellcheck source-path=SCRIPTDIR
source "$(dirname "${BASH_SOURCE[0]}")"/style.sh

fail() {
  err "$*"
  exit 1
}


quiet_assert_succeed() {
  local status=0
  bash -c "$1" || status=$?
  if [[ $status -ne 0 ]]; then
    fail "[$1] command failed with status $status"
  fi
}

assert_succeed() {
  if quiet_assert_succeed "$1"; then
    ok "[$1] expected success"
  fi
}

assert_fail() {
  if ! bash -c "$1" 2>&1; then
    ok "[$1] expected failure"
  else
    fail "[$1] expected failure but succeeded"
  fi
}

assert() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" == "$2" ]]; then
    ok "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert_not() {
  local actual
  actual="$(bash -c "$1" || true)"
  if [[ "$actual" != "$2" ]]; then
    ok "[$1] output is different from '$2'"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" == *"$2"* ]]; then
    ok "[$1] '$2' is in output"
  else
    fail "[$1] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" != *"$2"* ]]; then
    ok "[$1] '$2' is not in output"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_matches() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" =~ $2 ]]; then
    ok "[$1] '$2' matches output"
  else
    fail "[$1] expected '$2' to match '$actual'"
  fi
}

skip_slow_test() {
  if [[ -z "${TEST_ALL:-}" ]]; then
    warn "skipping slow tests"
    exit 0
  fi
}
