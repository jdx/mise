#!/usr/bin/env bash

# Shared safety boundary for destructive Homebrew differential tests. Callers
# must source this file, validate their exact platform, then call
# brew_oracle_require_disposable before any package or filesystem mutation.

brew_oracle_fail() {
  echo "brew oracle safety check failed: $*" >&2
  return 1
}

brew_oracle_validate_homebrew_identity() {
  local reference_version=$1 reference_sha=$2 runtime_version=$3 runtime_sha=$4 value

  for value in "$reference_version" "$runtime_version"; do
    if [[ -z $value || $value == *$'\n'* || $value == *$'\r'* ]]; then
      brew_oracle_fail "Homebrew versions must be nonempty single-line values"
      return 1
    fi
  done
  if [[ ! $reference_sha =~ ^[0-9a-f]{40}$ ]]; then
    brew_oracle_fail "exact Homebrew reference SHA is required"
    return 1
  fi
  if [[ $runtime_version == not-installed || $runtime_sha == not-installed ]]; then
    if [[ $runtime_version != not-installed || $runtime_sha != not-installed ]]; then
      brew_oracle_fail "Homebrew runtime version and SHA must both be not-installed"
      return 1
    fi
  elif [[ ! $runtime_sha =~ ^[0-9a-f]{40}$ ]]; then
    brew_oracle_fail "exact Homebrew runtime SHA is required"
    return 1
  fi
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
  # Docker bind mounts preserve host ownership. Trust only this exact checkout
  # for this one read-only identity command; never alter global Git config.
  checkout_sha=$(git -c safe.directory="$ROOT" -C "$ROOT" rev-parse HEAD) || {
    brew_oracle_fail "cannot resolve checkout SHA"
    return 1
  }
  if [[ $checkout_sha != "$MISE_BREW_ORACLE_MISE_SHA" ]]; then
    brew_oracle_fail "checkout SHA $checkout_sha does not match expected $MISE_BREW_ORACLE_MISE_SHA"
    return 1
  fi
  brew_oracle_validate_homebrew_identity \
    "${MISE_BREW_ORACLE_HOMEBREW_REFERENCE_VERSION:-}" \
    "${MISE_BREW_ORACLE_HOMEBREW_REFERENCE_SHA:-}" \
    "${MISE_BREW_ORACLE_HOMEBREW_RUNTIME_VERSION:-}" \
    "${MISE_BREW_ORACLE_HOMEBREW_RUNTIME_SHA:-}" || return 1

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
    printf 'homebrew_reference_version=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_VERSION"
    printf 'homebrew_reference_sha=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_SHA"
    printf 'homebrew_runtime_version=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_VERSION"
    printf 'homebrew_runtime_sha=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_SHA"
  } >"$tmp"
  mv "$tmp" "$BREW_ORACLE_MARKER"
}

brew_oracle_verify_marker() {
  local marker=$1 test_name=$2 fixture_count=$3 prefix=$4 mise_sha=$5
  local reference_version=$6 reference_sha=$7 runtime_version=$8 runtime_sha=$9

  if [[ ! -f $marker || -L $marker ]]; then
    brew_oracle_fail "missing completion marker: $marker"
    return 1
  fi
  if [[ ! $fixture_count =~ ^[1-9][0-9]*$ || ! $mise_sha =~ ^[0-9a-f]{40}$ ]]; then
    brew_oracle_fail "invalid expected fixture count or mise SHA"
    return 1
  fi
  brew_oracle_validate_homebrew_identity \
    "$reference_version" "$reference_sha" "$runtime_version" "$runtime_sha" || return 1
  if ! cmp -s "$marker" <(printf '%s\n' \
    "test_name=$test_name" \
    "fixture_count=$fixture_count" \
    "prefix=$prefix" \
    "mise_sha=$mise_sha" \
    "homebrew_reference_version=$reference_version" \
    "homebrew_reference_sha=$reference_sha" \
    "homebrew_runtime_version=$runtime_version" \
    "homebrew_runtime_sha=$runtime_sha"); then
    brew_oracle_fail "completion marker does not exactly match expected proof: $marker"
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
