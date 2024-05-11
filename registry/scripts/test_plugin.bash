#!/usr/bin/env bash

# See: ./scripts/test_plugin.bash --help

function fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Extract the link to a svg badge from a line of README.
function badge_svg() {
  grep -o '(http[^)]*svg[^)]*)' | sed -e 's/(//;s/)//'
}

function check_plugins_from_diff() {
  local BASE_REF="$1" # Base commit of main branch.
  local HEAD_REF="$2" # Latest commit of PR

  local DIFF_CHANGES
  DIFF_CHANGES="$(git diff --name-only "${BASE_REF}" "${HEAD_REF}")"

  # Assert that the PR diff only includes a change to README and file(s) under plugins/ directory
  {
    test 1 -lt "$(echo "$DIFF_CHANGES" | wc --lines | xargs)" &&
      echo "$DIFF_CHANGES" | grep README.md >/dev/null &&
      echo "$DIFF_CHANGES" | grep plugins/ >/dev/null
  } || fail "Expected git diff ${REF_RANGE} to only include changes for a single plugin"

  local PLUGIN_FILES
  PLUGIN_FILES="$(git diff --name-only "${BASE_REF}" "${HEAD_REF}" -- plugins/)"
  for PLUGIN_FILE in $PLUGIN_FILES; do
    echo "Checking $PLUGIN_FILE"
    check_plugin_from_file "$PLUGIN_FILE"
  done
}

function check_plugin_from_file() {
  local PLUGIN_FILE="$1"

  # Assert that we have a file at plugins/PLUGIN_NAME
  test -f "$PLUGIN_FILE" ||
    fail "Expected a plugin file at $PLUGIN_FILE"

  # Assert that PLUGIN_FILE file ends with new line.
  test 'X' == "$(tail --bytes=1 "$PLUGIN_FILE" | tr '\n' 'X')" ||
    fail "Expected $PLUGIN_FILE to end with a new line."

  # Assert that PLUGIN_FILE has repository key and points to a git repo.
  local PLUGIN_REPO
  PLUGIN_REPO="$(sed -e 's/repository = //' "$PLUGIN_FILE")"
  test -n "$PLUGIN_REPO" ||
    fail "File $PLUGIN_FILE does not specify a repository url"

  # Assert the plugin repo is reachable.
  git ls-remote --quiet "$PLUGIN_REPO" HEAD >/dev/null ||
    fail "Repo is unreachable: $PLUGIN_REPO"

  local PLUGIN_REPO_NO_GIT
  PLUGIN_REPO_NO_GIT="$(echo "$PLUGIN_REPO" | sed -e 's/\.git$//' | tr '/' '\n' | tail --lines 1)"

  local README_LINE
  README_LINE="$(git grep -h --ignore-case -C0 "${PLUGIN_REPO_NO_GIT}" -- README.md | head --lines 1)"

  # Assert that a new line for plugin was added at README
  test -n "$README_LINE" ||
    fail "Expected a line at README.md with a link to ${PLUGIN_REPO_NO_GIT}"

  echo "OK $PLUGIN_FILE"
}

function check_all_plugins() {
  local fails=0
  local total=0
  local out
  for file in plugins/*; do

    total=$((total + 1))
    out="$($0 --file "$file" 2>&1)"
    result=$?

    if test "${result}" == 0 && test ! "${CI}"; then
      # show successes locally, not in CI
      printf "* Checking %s %s\n" "$file" "[PASSED]"
    elif test "${result}" != 0; then
      # show failures locally and in CI
      fails=$((fails + 1))
      printf "* Checking %s %s\n" "$file" "[FAILED]"
      printf "*\t%s\n" "$out"
    fi
  done

  printf "\n"
  printf "%s %s\n" "Plugins available:" "${total}"
  printf "%s %s\n" "Plugin build checks passed:" "$((total - fails))"
  printf "%s %s\n" "Plugin build checks failed:" "${fails}"
  exit ${fails}
}

if test "--all" == "$*"; then
  printf "%s\n" "Testing all registered plugins"
  check_all_plugins

elif test "--diff" == "$1"; then
  printf "%s %s..%s" "Testing plugin introduced at git diff" "$2" "$3"
  check_plugins_from_diff "$2" "$3"

elif test "--file" == "$1"; then
  check_plugin_from_file "$2"
else
  cat <<-EOF
Test that a plugin at PLUGIN_FILE follows basic sanity checks:
  * A plugins/<plugin> file contains the repository url.
  * A new line is added on README.md with a link mentioning the plugin.
  * The new plugin has CI badge and it's passing.

Usage:

> $0 --file plugins/PLUGIN_FILE
    Test only plugin at file.

> $0 --all
    Test all plugins registered under plugins/

> $0 --diff BASE_REF HEAD_REF
    Test a single plugin introduced in git diff BASE_REF..HEAD_REF
EOF
fi
