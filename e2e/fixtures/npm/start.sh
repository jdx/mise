#!/usr/bin/env bash
# Source from a test to serve npm metadata from e2e/fixtures/npm/server.py
# instead of registry.npmjs.org. Both mise's HTTP client and `npm view` (under
# npm.shell_out) read the registry from NPM_CONFIG_REGISTRY.
#
# Call assert_fake_npm_registry_used at the end of the test so a lookup that
# bypassed the fake registry fails instead of silently reaching the network.

FAKE_NPM_REGISTRY_DIR="$PWD/.fake-npm-registry"
python3 "$ROOT/e2e/fixtures/npm/server.py" "$FAKE_NPM_REGISTRY_DIR" &
FAKE_NPM_REGISTRY_PID=$!
trap 'kill "$FAKE_NPM_REGISTRY_PID" 2>/dev/null || true' EXIT
for _ in {1..100}; do
  [[ -s "$FAKE_NPM_REGISTRY_DIR/port" ]] && break
  kill -0 "$FAKE_NPM_REGISTRY_PID" 2>/dev/null || fail "fake npm registry exited before listening"
  sleep 0.1
done
[[ -s "$FAKE_NPM_REGISTRY_DIR/port" ]] || fail "fake npm registry did not start"
NPM_CONFIG_REGISTRY="http://127.0.0.1:$(cat "$FAKE_NPM_REGISTRY_DIR/port")/"
export NPM_CONFIG_REGISTRY

# Asserts every named package was fetched from the fake registry.
assert_fake_npm_registry_used() {
  local package
  for package in "$@"; do
    grep -Fxq "$package" "$FAKE_NPM_REGISTRY_DIR/requests" 2>/dev/null ||
      fail "npm:$package was not fetched from the fake registry at $NPM_CONFIG_REGISTRY"
  done
}
