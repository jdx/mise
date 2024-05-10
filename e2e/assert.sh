#!/usr/bin/env bash

# Define "success" and "fail" with(out) coloring
if [[ ( -n ${CI:-} || -t 2 ) && -z ${NO_COLOR:-} ]]; then
  # Success in green
  succeed() {
    echo $'\e[92m'"SUCCESS: $*"$'\e[0m' >&2
  }
  # Failure in red
  fail() {
    echo $'\e[91m'"FAILURE: $*"$'\e[0m' >&2
    exit 1
  }
  # Skipped in yellow
  skip() {
    echo $'\e[93m'"SKIPPED: $*"$'\e[0m' >&2
    exit 0
  }
else
  succeed() {
    echo "SUCCESS: $*" >&2
  }
  fail() {
    echo "FAILURE: $*" >&2
    exit 1
  }
  skip() {
    echo "SKIPPED:$*" >&2
    exit 0
  }
fi

quiet_assert_succeed() {
  local status=0
  bash -c "$1" || status=$?
  if [[ $status -ne 0 ]]; then
    fail "[$1] command failed with status $status"
  fi
}

assert_succeed() {
  if quiet_assert_succeed "$1"; then
    succeed "[$1] expected success"
  fi
}

assert_fail() {
  if ! bash -c "$1" 2>&1; then
    succeed "[$1] expected failure"
  else
    fail "[$1] expected failure but succeeded"
  fi
}

assert() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" == "$2" ]]; then
    succeed "[$1] output is equal to '$2'"
  else
    fail "[$1] expected '$2' but got '$actual'"
  fi
}

assert_not() {
  local actual
  actual="$(bash -c "$1" || true)"
  if [[ "$actual" != "$2" ]]; then
    succeed "[$1] output is different from '$2'"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}


assert_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" == *"$2"* ]]; then
    succeed "[$1] '$2' is in output"
  else
    fail "[$1] expected '$2' to be in '$actual'"
  fi
}

assert_not_contains() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" != *"$2"* ]]; then
    succeed "[$1] '$2' is not in output"
  else
    fail "[$1] expected '$2' not to be in '$actual'"
  fi
}

assert_matches() {
  local actual
  actual="$(quiet_assert_succeed "$1")"
  if [[ "$actual" =~ $2 ]]; then
    succeed "[$1] '$2' matches output"
  else
    fail "[$1] expected '$2' to match '$actual'"
  fi
}
