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
  elif [[ $runtime_version != "$reference_version" || $runtime_sha != "$reference_sha" ]]; then
    brew_oracle_fail "Homebrew runtime must exactly match the pinned reference"
    return 1
  fi
}

brew_oracle_configure_runtime() {
  local expected_prefix=$1 brew_source repository bridge
  local brew_real repository_real runner_real prefix_real prefix_owner repository_owner
  local actual_prefix actual_repository actual_version actual_sha

  brew_source=${MISE_BREW_ORACLE_BREW_SOURCE:-}
  repository=${MISE_BREW_ORACLE_HOMEBREW_REPOSITORY:-}
  if [[ $brew_source != /* || $repository != /* ]]; then
    brew_oracle_fail "absolute pinned Homebrew source and repository are required"
    return 1
  fi
  if [[ ! -f $brew_source || ! -x $brew_source || -L $brew_source ]]; then
    brew_oracle_fail "pinned Homebrew source must be an executable non-symlink file"
    return 1
  fi
  if [[ ! -d $repository || -L $repository ]]; then
    brew_oracle_fail "pinned Homebrew repository must be a non-symlink directory"
    return 1
  fi

  brew_real=$(realpath "$brew_source") || return 1
  repository_real=$(realpath "$repository") || return 1
  runner_real=$(realpath "$MISE_BREW_ORACLE_RUNNER_TEMP") || return 1
  prefix_real=$(realpath "$expected_prefix") || return 1
  [[ $brew_real == "$repository_real/bin/brew" ]] || {
    brew_oracle_fail "Homebrew source is not owned by the pinned repository"
    return 1
  }
  case "$repository_real/" in
    "$runner_real"/* | "$prefix_real/Homebrew/") ;;
    *)
      brew_oracle_fail "Homebrew repository is outside the disposable runner or canonical prefix"
      return 1
      ;;
  esac

  if [[ -L $expected_prefix || -L $expected_prefix/bin ]]; then
    brew_oracle_fail "canonical Homebrew prefix and bin directory must not be symlinks"
    return 1
  fi
  if [[ $(uname) == Darwin ]]; then
    prefix_owner=$(stat -f '%u' "$prefix_real")
    repository_owner=$(stat -f '%u' "$repository_real")
  else
    prefix_owner=$(stat -c '%u' "$prefix_real")
    repository_owner=$(stat -c '%u' "$repository_real")
  fi
  [[ $prefix_owner == "$repository_owner" ]] || {
    brew_oracle_fail "pinned repository and canonical prefix have different owners"
    return 1
  }

  actual_sha=$(git -C "$repository_real" rev-parse HEAD) || return 1
  [[ $actual_sha == "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_SHA" ]] || {
    brew_oracle_fail "pinned repository SHA $actual_sha does not match the marker identity"
    return 1
  }
  bridge="$expected_prefix/bin/mise-brew-oracle-$actual_sha"
  if [[ -e $bridge || -L $bridge ]]; then
    brew_oracle_fail "stale pinned Homebrew bridge exists: $bridge"
    return 1
  fi
  ln -s "$brew_real" "$bridge" || return 1
  BREW_ORACLE_RUNTIME_BRIDGE=$bridge

  export HOMEBREW_NO_ANALYTICS=1
  export HOMEBREW_NO_AUTO_UPDATE=1
  export HOMEBREW_NO_ENV_HINTS=1
  export HOMEBREW_PREFIX=$prefix_real
  export HOMEBREW_REPOSITORY=$repository_real
  export HOMEBREW_LIBRARY=$repository_real/Library

  actual_prefix=$("$bridge" --prefix) || {
    brew_oracle_remove_runtime_bridge
    return 1
  }
  actual_repository=$("$bridge" --repository) || {
    brew_oracle_remove_runtime_bridge
    return 1
  }
  actual_version=$("$bridge" --version | awk 'NR == 1 { print $2 }') || {
    brew_oracle_remove_runtime_bridge
    return 1
  }
  [[ $(realpath "$actual_prefix") == "$prefix_real" ]] || {
    brew_oracle_fail "runtime Homebrew prefix $actual_prefix does not match $prefix_real"
    brew_oracle_remove_runtime_bridge
    return 1
  }
  [[ $(realpath "$actual_repository") == "$repository_real" ]] || {
    brew_oracle_fail "runtime Homebrew repository does not match the pinned checkout"
    brew_oracle_remove_runtime_bridge
    return 1
  }
  [[ $actual_version == "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_RELEASE" ]] || {
    brew_oracle_fail "runtime Homebrew version $actual_version does not match the marker identity"
    brew_oracle_remove_runtime_bridge
    return 1
  }
  [[ $actual_sha == "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_SHA" ]] || {
    brew_oracle_fail "runtime Homebrew SHA $actual_sha does not match the marker identity"
    brew_oracle_remove_runtime_bridge
    return 1
  }

  export BREW_ORACLE_BREW=$bridge
}

brew_oracle_remove_runtime_bridge() {
  local bridge=${BREW_ORACLE_RUNTIME_BRIDGE:-} expected_source=${MISE_BREW_ORACLE_BREW_SOURCE:-}
  [[ -n $bridge ]] || return 0
  if [[ ! -L $bridge || $(readlink "$bridge") != "$expected_source" ]]; then
    brew_oracle_fail "refusing to remove ambiguous Homebrew runtime bridge: $bridge"
    return 1
  fi
  rm "$bridge"
  unset BREW_ORACLE_RUNTIME_BRIDGE BREW_ORACLE_BREW
}

brew_oracle_require_disposable() {
  local marker_name=$1 expected_prefix=$2
  local runner_temp result_dir runner_real result_real checkout_sha credential_name

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
    "${MISE_BREW_ORACLE_HOMEBREW_REFERENCE_RELEASE:-}" \
    "${MISE_BREW_ORACLE_HOMEBREW_REFERENCE_SHA:-}" \
    "${MISE_BREW_ORACLE_HOMEBREW_RUNTIME_RELEASE:-}" \
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
  for credential_name in GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN; do
    if [[ -n ${!credential_name:-} ]]; then
      brew_oracle_fail "CI credential leaked into destructive oracle: $credential_name"
      return 1
    fi
  done

  BREW_ORACLE_EXPECTED_PREFIX=$expected_prefix
  BREW_ORACLE_MARKER="$result_real/$marker_name.completed"
  if [[ -e $BREW_ORACLE_MARKER || -L $BREW_ORACLE_MARKER ]]; then
    brew_oracle_fail "stale completion marker exists: $BREW_ORACLE_MARKER"
    return 1
  fi
}

brew_oracle_complete() {
  local test_name=$1 fixture_count=$2 actual_prefix=$3 tmp
  local result_uid=${MISE_BREW_ORACLE_RESULT_UID:-}
  local result_gid=${MISE_BREW_ORACLE_RESULT_GID:-}

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
    printf 'homebrew_reference_version=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_RELEASE"
    printf 'homebrew_reference_sha=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_SHA"
    printf 'homebrew_runtime_version=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_RELEASE"
    printf 'homebrew_runtime_sha=%s\n' "$MISE_BREW_ORACLE_HOMEBREW_RUNTIME_SHA"
  } >"$tmp"
  if [[ -n $result_uid || -n $result_gid ]]; then
    if [[ ! $result_uid =~ ^[0-9]+$ || ! $result_gid =~ ^[0-9]+$ ]]; then
      rm -f "$tmp"
      brew_oracle_fail "oracle result owner must be numeric"
      return 1
    fi
    chown "$result_uid:$result_gid" "$tmp" || {
      rm -f "$tmp"
      brew_oracle_fail "cannot transfer completion record ownership"
      return 1
    }
  fi
  chmod 0644 "$tmp" || {
    rm -f "$tmp"
    brew_oracle_fail "cannot make completion record readable"
    return 1
  }
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
