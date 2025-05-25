#!/usr/bin/env bash

# shellcheck source-path=SCRIPTDIR
source "$TEST_ROOT/style.sh"

fail() {
  title="E2E assertion failed" err "$*"
  exit 1
}

# Safeguard against running the test directly, which would execute in the actual user home
[[ -n ${TEST_NAME:-} ]] || fail "tests should be called using run_test"

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
  if [[ -z ${2:-} ]]; then
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
  if [[ -z ${2:-} ]]; then
    ok "[$1]"
  elif [[ $actual == "$2" ]]; then
    ok "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert_json() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if jq -e . >/dev/null <<<"$actual"; then
    ok "[$1] output is valid JSON"
  else
    fail "[$1] output is not valid JSON"
  fi

  actual_json="$(jq . <<<"$actual")"
  expected_json="$(jq . <<<"$2")"
  if [[ $actual_json == "$expected_json" ]]; then
    ok "[$1] output is equal to '$2'"
  else
    diff --side-by-side <(jq . <<<"$expected_json") <(jq . <<<"$actual_json") || true
    fail "JSON output from [$1] is different from expected"
  fi
}

assert_json_partial_array() {
  local command="$1" fields="$2" expected="$3"

  local actual
  actual="$(quiet_assert_succeed "$command")"

  local filter="map({$fields})"
  local actual_filtered expected_filtered

  actual_filtered="$(jq -S "$filter" <<<"$actual")"
  expected_filtered="$(jq -S "$filter" <<<"$expected")"

  if [[ $actual_filtered == "$expected_filtered" ]]; then
    ok "[$command] partial array match successful"
  else
    echo "Expected:"
    echo "$expected_filtered"
    echo "Got:"
    echo "$actual_filtered"
    fail "[$command] partial array match failed"
  fi
}

assert_json_partial_object() {
  local command="$1" fields="$2" expected="$3"

  local actual
  actual="$(quiet_assert_succeed "$command")"

  # shellcheck disable=SC2016
  local filter='with_entries(select(.key as $k | ($fields | split(",")) | contains([$k])))'
  local actual_filtered expected_filtered

  actual_filtered="$(jq -S --arg fields "$fields" "$filter" <<<"$actual")"
  expected_filtered="$(jq -S --arg fields "$fields" "$filter" <<<"$expected")"

  if [[ $actual_filtered == "$expected_filtered" ]]; then
    ok "[$command] partial object match successful"
  else
    echo "Expected:"
    echo "$expected_filtered"
    echo "Got:"
    echo "$actual_filtered"
    fail "[$command] partial object match failed"
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

assert_empty() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ -z $actual ]]; then
    ok "[$1] output is empty"
  else
    fail "[$1] expected empty output but got '$actual'"
  fi
}

assert_directory_exists() {
  if [[ -d $1 ]]; then
    ok "[$1] directory exists"
  else
    fail "[$1] directory does not exist"
  fi
}

assert_directory_not_exists() {
  if [[ ! -d $1 ]]; then
    ok "[$1] directory does not exist"
  else
    fail "[$1] directory exists"
  fi
}

assert_directory_empty() {
  if [[ -z "$(ls -A "$1")" ]]; then
    ok "[$1] directory is empty"
  else
    fail "[$1] directory is not empty"
  fi
}

assert_directory_not_empty() {
  if [[ -n "$(ls -A "$1")" ]]; then
    ok "[$1] directory is not empty"
  else
    fail "[$1] directory is empty"
  fi
}

require_cmd() {
  if ! type -p "$1" >/dev/null; then
    title="E2E test $TEST_NAME aborted" err "'$1' is required but was not found in PATH"
    exit 2
  fi
}
