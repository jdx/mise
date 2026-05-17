#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

resolve_mise_bin() {
  if [[ -n ${MISE_E2E_BIN:-} ]]; then
    printf '%s\n' "$MISE_E2E_BIN"
    return
  fi
  local candidate="${CARGO_TARGET_DIR:-$ROOT/target}/debug/mise"
  if [[ -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return
  fi
  local mise_path
  if ! mise_path="$(command -v mise 2>/dev/null)"; then
    err "Could not find 'mise' in PATH and no local debug build was found. Set MISE_E2E_BIN or build/install mise."
    return 1
  fi
  printf '%s\n' "$mise_path"
}
