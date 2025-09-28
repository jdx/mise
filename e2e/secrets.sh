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
TEST_SECRET = { secret = { provider = "env" } }
TEST_WITH_KEY = { secret = { provider = "env", key = "HOME" } }
EOF

# Test that secrets check works with env provider
export TEST_SECRET="test_value"
$MISE secrets check
echo "✓ secrets check passed"

# Test that secrets get works
output=$($MISE secrets get TEST_SECRET --provider env --show)
if [ "$output" != "test_value" ]; then
	echo "✗ Expected 'test_value', got '$output'"
	exit 1
fi
echo "✓ secrets get returned correct value"

# Test redacted output
output=$($MISE secrets get TEST_SECRET --provider env 2>/dev/null)
if [[ $output != *"<redacted>"* ]]; then
	echo "✗ Expected redacted output, got '$output'"
	exit 1
fi
echo "✓ secrets get redacts by default"

# Test that env resolution works with secrets
output=$($MISE env | grep TEST_SECRET || echo "")
if [ -z "$output" ]; then
	echo "✗ Secret not resolved in env"
	exit 1
fi
echo "✓ secrets resolved in env"

# Test key mapping
output=$($MISE secrets get TEST_WITH_KEY --provider env --show)
if [ "$output" != "$HOME" ]; then
	echo "✗ Expected '$HOME', got '$output'"
	exit 1
fi
echo "✓ key mapping works"

# Clean up
cd /
rm -rf "$TEST_DIR"

echo "All secret tests passed!"
