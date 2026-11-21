#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Prints the mise binary the tests run: MISE_E2E_BIN when set, otherwise this
# checkout's debug build. There is no fallback to a mise found on PATH, because
# that one does not contain the change being tested and the run would still
# pass. A `target` symlink whose destination is gone counts as no build.
resolve_mise_bin() {
  if [[ -n ${MISE_E2E_BIN:-} ]]; then
    if [[ ! -f $MISE_E2E_BIN || ! -x $MISE_E2E_BIN ]]; then
      err "MISE_E2E_BIN is set to '$MISE_E2E_BIN', which is not an executable file."
      return 1
    fi
    # Outside Docker the tests find the binary by name on PATH. A Docker run
    # mounts the file as `mise`, so any name works there.
    if [[ ${MISE_E2E_DOCKER:-0} != 1 && $(basename "$MISE_E2E_BIN") != mise ]]; then
      err "MISE_E2E_BIN is set to '$MISE_E2E_BIN', but the tests call the binary by name: the file must be named 'mise'."
      return 1
    fi
    printf '%s\n' "$MISE_E2E_BIN"
    return
  fi
  local candidate="${CARGO_TARGET_DIR:-$ROOT/target}/debug/mise"
  if [[ -f $candidate && -x $candidate ]]; then
    printf '%s\n' "$candidate"
    return
  fi
  err "No mise build at '$candidate'. Run 'mise run build' first, or set MISE_E2E_BIN=/path/to/mise to test another mise binary on purpose."
  return 1
}
