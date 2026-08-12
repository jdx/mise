#!/usr/bin/env bash

# Shared safety boundary for destructive Homebrew differential tests. Callers
# must source this file, validate their exact platform, then call
# brew_oracle_require_disposable before any package or filesystem mutation.

brew_oracle_fail() {
  echo "brew oracle safety check failed: $*" >&2
  return 1
}

brew_oracle_require_disposable() {
  local marker_name=$1 expected_prefix=$2
  local runner_temp result_dir runner_real result_real checkout_sha

  if [[ ${MISE_BREW_ORACLE_DISPOSABLE:-} != 1 ]]; then
    brew_oracle_fail "MISE_BREW_ORACLE_DISPOSABLE=1 is required"
    return 1
  fi

  runner_temp=${MISE_BREW_ORACLE_RUNNER_TEMP:-}
  result_dir=${MISE_BREW_ORACLE_RESULT_DIR:-}
  if [[ $runner_temp != /* ]]; then
    brew_oracle_fail "runner temp must be absolute"
    return 1
  fi
  if [[ $result_dir != /* ]]; then
    brew_oracle_fail "result directory must be absolute"
    return 1
  fi
  if [[ ! -d $runner_temp ]]; then
    brew_oracle_fail "runner temp must be an existing directory"
    return 1
  fi
  if [[ ! -d $result_dir || -L $result_dir ]]; then
    brew_oracle_fail "result directory must be an existing non-symlink directory"
    return 1
  fi

  runner_real=$(realpath "$runner_temp") || {
    brew_oracle_fail "cannot resolve runner temp"
    return 1
  }
  result_real=$(realpath "$result_dir") || {
    brew_oracle_fail "cannot resolve result directory"
    return 1
  }
  case "$result_real/" in
    "$runner_real"/*) ;;
    *)
      brew_oracle_fail "result directory escapes runner temp"
      return 1
      ;;
  esac

  if [[ ! $marker_name =~ ^[a-z0-9-]+$ ]]; then
    brew_oracle_fail "invalid completion marker name"
    return 1
  fi
  if [[ $expected_prefix != /* ]]; then
    brew_oracle_fail "expected prefix must be absolute"
    return 1
  fi
  if [[ ! ${MISE_BREW_ORACLE_MISE_SHA:-} =~ ^[0-9a-f]{40}$ ]]; then
    brew_oracle_fail "exact mise SHA is required"
    return 1
  fi
  checkout_sha=$(git -C "$ROOT" rev-parse HEAD) || {
    brew_oracle_fail "cannot resolve checkout SHA"
    return 1
  }
  if [[ $checkout_sha != "$MISE_BREW_ORACLE_MISE_SHA" ]]; then
    brew_oracle_fail "checkout SHA $checkout_sha does not match expected $MISE_BREW_ORACLE_MISE_SHA"
    return 1
  fi

  # These are deliberately set by the workflow but must not cross env -i.
  if [[ -n ${CI+x} ]]; then
    brew_oracle_fail "generic CI leaked through the e2e sanitizer"
    return 1
  fi
  if [[ -n ${MISE_BREW_ORACLE_UNFORWARDED+x} ]]; then
    brew_oracle_fail "a non-allowlisted variable leaked through the e2e sanitizer"
    return 1
  fi

  BREW_ORACLE_EXPECTED_PREFIX=$expected_prefix
  BREW_ORACLE_MARKER="$result_real/$marker_name.completed"
  if [[ -e $BREW_ORACLE_MARKER || -L $BREW_ORACLE_MARKER ]]; then
    brew_oracle_fail "stale completion marker exists: $BREW_ORACLE_MARKER"
    return 1
  fi
}

brew_oracle_complete() {
  local test_name=$1 fixture_count=$2 actual_prefix=$3 tmp

  if [[ -z ${BREW_ORACLE_MARKER:-} ]]; then
    brew_oracle_fail "oracle was not preflighted"
    return 1
  fi
  if [[ ! $fixture_count =~ ^[1-9][0-9]*$ ]]; then
    brew_oracle_fail "fixture count must be positive"
    return 1
  fi
  if [[ $actual_prefix != "$BREW_ORACLE_EXPECTED_PREFIX" ]]; then
    brew_oracle_fail "actual prefix $actual_prefix does not match $BREW_ORACLE_EXPECTED_PREFIX"
    return 1
  fi

  tmp=$(mktemp "${BREW_ORACLE_MARKER}.tmp.XXXXXX") || {
    brew_oracle_fail "cannot create completion record"
    return 1
  }
  {
    printf 'test_name=%s\n' "$test_name"
    printf 'fixture_count=%s\n' "$fixture_count"
    printf 'prefix=%s\n' "$actual_prefix"
    printf 'mise_sha=%s\n' "$MISE_BREW_ORACLE_MISE_SHA"
  } >"$tmp"
  mv "$tmp" "$BREW_ORACLE_MARKER"
}

brew_oracle_verify_marker() {
  local marker=$1 test_name=$2 fixture_count=$3 prefix=$4 mise_sha=$5

  if [[ ! -f $marker || -L $marker ]]; then
    brew_oracle_fail "missing completion marker: $marker"
    return 1
  fi
  if ! grep -Fxq "test_name=$test_name" "$marker"; then
    brew_oracle_fail "wrong test name in $marker"
    return 1
  fi
  if ! grep -Fxq "fixture_count=$fixture_count" "$marker"; then
    brew_oracle_fail "wrong fixture count in $marker"
    return 1
  fi
  if ! grep -Fxq "prefix=$prefix" "$marker"; then
    brew_oracle_fail "wrong prefix in $marker"
    return 1
  fi
  if ! grep -Fxq "mise_sha=$mise_sha" "$marker"; then
    brew_oracle_fail "wrong mise SHA in $marker"
    return 1
  fi
}

brew_oracle_require_absent_path() {
  local path=$1 description=$2

  if [[ -e $path || -L $path ]]; then
    brew_oracle_fail "$description exists: $path"
    return 1
  fi
}
