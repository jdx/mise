#!/usr/bin/env bash

# shellcheck source-path=SCRIPTDIR
source "$TEST_ROOT/style.sh"

fail() {
  title="E2E assertion failed" err "$*"
  exit 1
}

# Safeguard against running the test directly, which would execute in the actual user home
[[ -n "${TEST_NAME:-}" ]] || fail "tests should be called using run_test"

quiet_assert_succeed() {
  local status=0
  debug "$ $1"
  bash -c "$1" || status=$?
  if [[ $status -ne 0 ]]; then
    fail "[$1] command failed with status $status"
  fi
}
quiet_assert_fail() {
  local status=0
  debug "$ $1"
  MISE_FRIENDLY_ERROR=1 RUST_BACKTRACE=0 bash -c "$1 2>&1" || status=$?
  if [[ $status -eq 0 ]]; then
    fail "[$1] command succeeded but was expected to fail"
  fi
}

assert_succeed() {
  if quiet_assert_succeed "$1"; then
    ok "[$1] expected success"
  fi
}

assert_fail() {
  local actual
  actual="$(quiet_assert_fail "$1")"
  if [[ -z "${2:-}" ]]; then
    ok "[$1] expected failure"
  elif [[ $actual == *"$2"* ]]; then
    ok "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ -z "${2:-}" ]]; then
    ok "[$1]"
  elif [[ $actual == "$2" ]]; then
    ok "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert_not() {
  local actual
  debug "$ $1"
  actual="$(bash -c "$1" || true)"
  if [[ $actual != "$2" ]]; then
    ok "[$1] output is different from '$2'"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual == *"$2"* ]]; then
    ok "[$1] '$2' is in output"
  else
    fail "[$1] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual != *"$2"* ]]; then
    ok "[$1] '$2' is not in output"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_matches() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ $actual =~ $2 ]]; then
    ok "[$1] '$2' matches output"
  else
    fail "[$1] expected '$2' to match '$actual'"
  fi
}

require_cmd() {
  if ! type -p "$1" >/dev/null; then
    title="E2E test $TEST_NAME aborted" err "'$1' is required but was not found in PATH"
    exit 2
  fi
}
