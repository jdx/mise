#!/usr/bin/env bash

# Safety boundary for destructive Homebrew tests. CI owns exact-head and
# pinned-image checks; this guard proves the mutation authority and target.

brew_oracle_fail() {
  echo "brew formula test safety check failed: $*" >&2
  return 1
}

brew_oracle_require_disposable() {
  local test_name=$1 expected_prefix=$2 checkout_sha credential_name runner_real
  local probe probe_real suffix component source_real

  [[ ${MISE_BREW_ORACLE_DISPOSABLE:-} == 1 ]] ||
    brew_oracle_fail "MISE_BREW_ORACLE_DISPOSABLE=1 is required" || return 1
  [[ $expected_prefix == /* ]] ||
    brew_oracle_fail "expected prefix must be absolute" || return 1
  [[ ${MISE_BREW_ORACLE_RUNNER_TEMP:-} == /* ]] ||
    brew_oracle_fail "runner temp must be absolute" || return 1
  [[ -d $MISE_BREW_ORACLE_RUNNER_TEMP && ! -L $MISE_BREW_ORACLE_RUNNER_TEMP ]] ||
    brew_oracle_fail "runner temp must be a real directory" || return 1

  checkout_sha=$(git -c safe.directory="$ROOT" -C "$ROOT" rev-parse HEAD) || return 1
  [[ $checkout_sha == "${MISE_BREW_ORACLE_MISE_SHA:-}" ]] ||
    brew_oracle_fail "checkout does not match the requested head" || return 1

  for credential_name in GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN FORGEJO_TOKEN; do
    [[ -z ${!credential_name:-} ]] ||
      brew_oracle_fail "credential reached destructive test: $credential_name" || return 1
  done
  [[ -z ${CI+x} && -z ${MISE_BREW_ORACLE_UNFORWARDED+x} ]] ||
    brew_oracle_fail "non-allowlisted CI state reached destructive test" || return 1

  case "$test_name:$expected_prefix" in
    macos-formula-lifecycle:/opt/homebrew | linux-formula:/home/linuxbrew/.linuxbrew) ;;
    linux-source:*)
      runner_real=$(realpath "$MISE_BREW_ORACLE_RUNNER_TEMP") || return 1
      case "/$expected_prefix/" in
        */../* | */./*) brew_oracle_fail "source prefix must not contain dot components" || return 1 ;;
      esac
      probe=$expected_prefix
      suffix=
      while [[ ! -e $probe && ! -L $probe ]]; do
        component=${probe##*/}
        suffix="/$component$suffix"
        probe=${probe%/*}
        [[ -n $probe ]] || probe=/
      done
      [[ -d $probe && ! -L $probe ]] ||
        brew_oracle_fail "source prefix ancestry must be real directories" || return 1
      probe_real=$(realpath "$probe") || return 1
      source_real="$probe_real$suffix"
      case "$source_real/" in
        "$runner_real"/*) ;;
        *) brew_oracle_fail "source prefix must be inside runner temp" || return 1 ;;
      esac
      ;;
    *) brew_oracle_fail "unexpected destructive test target" || return 1 ;;
  esac

  export BREW_ORACLE_EXPECTED_PREFIX=$expected_prefix
}

brew_oracle_configure_runtime() {
  local expected_prefix=$1 brew_source repository brew_real repository_real runner_real
  local prefix_real actual_sha actual_version bridge

  brew_source=${MISE_BREW_ORACLE_BREW_SOURCE:-}
  repository=${MISE_BREW_ORACLE_HOMEBREW_REPOSITORY:-}
  [[ $brew_source == /* && $repository == /* ]] ||
    brew_oracle_fail "absolute pinned Homebrew paths are required" || return 1
  [[ -x $brew_source && ! -L $brew_source && -d $repository && ! -L $repository ]] ||
    brew_oracle_fail "pinned Homebrew checkout is invalid" || return 1

  brew_real=$(realpath "$brew_source") || return 1
  repository_real=$(realpath "$repository") || return 1
  runner_real=$(realpath "$MISE_BREW_ORACLE_RUNNER_TEMP") || return 1
  prefix_real=$(realpath "$expected_prefix") || return 1
  [[ $brew_real == "$repository_real/bin/brew" ]] ||
    brew_oracle_fail "brew does not belong to the pinned checkout" || return 1
  case "$repository_real/" in
    "$runner_real"/* | "$prefix_real/Homebrew/") ;;
    *) brew_oracle_fail "pinned checkout is outside trusted roots" || return 1 ;;
  esac
  [[ ! -L $expected_prefix && ! -L $expected_prefix/bin ]] ||
    brew_oracle_fail "canonical prefix must not be symlinked" || return 1

  actual_sha=$(git -C "$repository_real" rev-parse HEAD) || return 1
  actual_version=$(env \
    HOMEBREW_PREFIX="$prefix_real" \
    HOMEBREW_REPOSITORY="$repository_real" \
    HOMEBREW_LIBRARY="$repository_real/Library" \
    "$brew_real" --version | awk 'NR == 1 { print $2 }') || return 1
  [[ $actual_sha == "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_SHA" &&
    $actual_version == "$MISE_BREW_ORACLE_HOMEBREW_REFERENCE_RELEASE" ]] ||
    brew_oracle_fail "Homebrew runtime does not match the pinned reference" || return 1

  bridge="$expected_prefix/bin/mise-brew-formula-test-$actual_sha"
  [[ ! -e $bridge && ! -L $bridge ]] ||
    brew_oracle_fail "stale Homebrew test bridge exists" || return 1
  ln -s "$brew_real" "$bridge" || return 1
  BREW_ORACLE_RUNTIME_BRIDGE=$bridge
  BREW_ORACLE_BREW=$bridge
  export BREW_ORACLE_BREW HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_AUTO_UPDATE=1
  export HOMEBREW_NO_ENV_HINTS=1 HOMEBREW_PREFIX=$prefix_real
  export HOMEBREW_REPOSITORY=$repository_real HOMEBREW_LIBRARY=$repository_real/Library
}

brew_oracle_remove_runtime_bridge() {
  local bridge=${BREW_ORACLE_RUNTIME_BRIDGE:-}
  [[ -n $bridge ]] || return 0
  [[ -L $bridge && $(readlink "$bridge") == "${MISE_BREW_ORACLE_BREW_SOURCE:-}" ]] ||
    brew_oracle_fail "refusing to remove ambiguous Homebrew bridge" || return 1
  rm "$bridge"
  unset BREW_ORACLE_RUNTIME_BRIDGE BREW_ORACLE_BREW
}

brew_oracle_require_absent_path() {
  local path=$1 description=$2
  [[ ! -e $path && ! -L $path ]] ||
    brew_oracle_fail "$description exists: $path" || return 1
}
