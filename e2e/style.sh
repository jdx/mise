# shellcheck shell=bash

if [[ -n ${GITHUB_ACTION:-} ]]; then
  # Output Github action annotations
  annotate() {
    : "${file:=${TEST_SCRIPT:-}}"
    : "${title:=}"
    echo "::${type:?}${file:+ file=${file}}${title:+ title=${title}}::$*" >&2
  }
  err() { type=error annotate "$*"; }
  warn() { type=warning annotate "$*"; }
  start_group() { echo "::group::$*" >&2; }
  end_group() { echo ::endgroup:: >&2; }

  # Yet use ANSI green color for the "ok" message
  ok() { echo $'\e[92m'"$*"$'\e[0m' >&2; }

elif [[ -t 2 ]]; then
  # Use ANSI coloring in terminal
  ok() { echo $'\e[92m'"$*"$'\e[0m' >&2; }
  err() { echo $'\e[91m'"$*"$'\e[0m' >&2; }
  warn() { echo $'\e[93m'"$*"$'\e[0m' >&2; }
  start_group() { echo $'\e[1m'">>> $*"$'\e[0m' >&2; }
  end_group() { echo >&2; }

else
  # No styling
  ok() { echo "SUCCESS: $*" >&2; }
  err() { echo "ERROR: $*" >&2; }
  warn() { echo "wARNING: $*" >&2; }
  start_group() { echo ">>> $*" >&2; }
  end_group() { echo >&2; }
fi

as_group() {
  local status=0
  start_group "$1"
  shift
  "$*" || status=$?
  end_group
  return "$status"
}
