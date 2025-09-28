#!/usr/bin/env bash
# Test secret functionality

set -euo pipefail

# Use the built binary
MISE=/Users/jdx/src/mise/target/debug/mise

# Create a temp dir for the test
TEST_DIR=$(mktemp -d)
cd "$TEST_DIR"

# Test basic secret configuration parsing
cat <<EOF >mise.toml
[env]
TEST_SECRET = { required = true }
TEST_OP_SECRET = { onepassword = { reference = "op://MyVault/Item/field" } }
TEST_KEYRING_SECRET = { keyring = { service = "test-service", account = "test-account" } }
EOF

# Also create a local config for the required env var
cat <<EOF >mise.local.toml
[env]
TEST_SECRET = "test_value"
EOF

# Test that required env var works
output=$($MISE env | grep TEST_SECRET || echo "")
if [ -z "$output" ]; then
	echo "✗ Required env var not resolved"
	exit 1
fi
echo "✓ required env var resolved from local config"

# Test that the env var shows up in env output
output=$($MISE env | grep "TEST_SECRET=test_value" || echo "")
if [ -z "$output" ]; then
	echo "✗ TEST_SECRET not found in env output"
	exit 1
fi
echo "✓ required env var shows correct value in env"

# Clean up
cd /
rm -rf "$TEST_DIR"

echo "All secret tests passed!"
