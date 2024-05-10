# shellcheck shell=bash

if [[ -n ${GITHUB_ACTION:-} ]]; then
  # Use special GA formatting
  # Use ANSI green color for the "ok" message so groups with no errors are kept collapsed
  _STYLE_OK=$'\e[92m'
  _STYLE_ERR='::error::'
  _STYLE_WARN='::warning::'
  _STYLE_RESET=$'\e[0m'
  _GROUP_START='::group::'
  _GROUP_END='::endgroup::'
elif [[ -t 2 ]]; then
  # Use ANSI coloring in terminal
  _STYLE_OK=$'\e[92m'       # green
  _STYLE_ERR=$'\e[91m'      # red
  _STYLE_WARN=$'\e[93m'     # yellow
  _STYLE_RESET=$'\e[0m'     # full reset
  _GROUP_START=$'\e[1m>>> ' # bold
  _GROUP_END=
else
  # No styling
  _STYLE_OK='SUCCESS: '
  _STYLE_ERR='ERROR: '
  _STYLE_WARN='NOTICE: '
  _STYLE_RESET=
  _GROUP_START='>>> '
  _GROUP_END=
fi

ok() {
  echo "${_STYLE_OK}$*${_STYLE_RESET}" >&2
}

err() {
  echo "${_STYLE_ERR}$*${_STYLE_RESET}" >&2
}

warn() {
  echo "${_STYLE_WARN}$*${_STYLE_RESET}" >&2
}

as_group() {
  local status=0
  echo "${_GROUP_START}$1${_STYLE_RESET}" >&2
  shift
  "$*" || status=$?
  echo "${_GROUP_END}${_STYLE_RESET}" >&2
  return "$status"
}
