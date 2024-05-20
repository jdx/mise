#!/usr/bin/env bash

if [[ -n ${GITHUB_ACTION:-} ]]; then
  # Output Github action annotations
  annotate() {
    local parameters=""
    [[ -n ${file:=${TEST_SCRIPT:-}} ]] && parameters="file=${file}"
    [[ -n ${title:-} ]] && parameters="${parameters:+,}title=${title}"
    echo "::${type:-debug}${parameters:+ ${parameters}}::$*" >&2
  }
  err() { type=error annotate "$*"; }
  warn() { type=warning annotate "$*"; }
  notice() { type=notice annotate "$*"; }
  # debug() { type=debug annotate "$*"; }
  debug() { echo $'\e[90m'"$*"$'\e[0m' >&2; }
  start_group() { echo "::group::$*" >&2; }
  end_group() { echo ::endgroup:: >&2; }

  # Yet use ANSI green color for the "ok" message
  ok() { echo $'\e[92m'"$*"$'\e[0m' >&2; }

elif [[ -t 2 ]]; then
  # Use ANSI coloring in terminal
  ok() { echo $'\e[92m'"$*"$'\e[0m' >&2; }
  err() { echo $'\e[91m'"${title:+$title: }$*"$'\e[0m' >&2; }
  warn() { echo $'\e[93m'"${title:+$title: }$*"$'\e[0m' >&2; }
  notice() { echo $'\e[94m'"$*"$'\e[0m' >&2; }
  debug() { echo $'\e[90m'"$*"$'\e[0m' >&2; }
  start_group() { echo $'\e[1m'">>> $*"$'\e[0m' >&2; }
  end_group() { echo >&2; }

else
  # No styling
  ok() { echo "SUCCESS: $*" >&2; }
  err() { echo "ERROR: ${title:+$title: }$*" >&2; }
  warn() { echo "WARNING: ${title:+$title: }$*" >&2; }
  notice() { echo "NOTICE: $*" >&2; }
  debug() { echo "DEBUG: $*" >&2; }
  start_group() { echo ">>> $*" >&2; }
  end_group() { echo >&2; }
fi

if [[ -n ${GITHUB_STEP_SUMMARY:-} ]]; then
  summary() { echo "| $1 | $2 | $3 |" >>"$GITHUB_STEP_SUMMARY"; }
else
  summary() { true; }
fi

as_group() {
  local status=0
  start_group "$1"
  shift
  "$*" || status=$?
  end_group
  return "$status"
}
