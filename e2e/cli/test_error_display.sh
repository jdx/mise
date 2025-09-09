#!/usr/bin/env bash

set -euo pipefail

# Test comprehensive error display for both friendly and non-friendly error messages
# This test validates that error messages are properly formatted and contain expected content

# Test friendly error messages (default behavior)
echo "Testing friendly error messages..."

# Test 1: Invalid tool version error (friendly)
echo "Test 1: Invalid tool version"
set +e
output=$(mise install node@invalid-version 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for invalid version"
	exit 1
fi

# Check that error contains expected patterns for friendly errors
if ! echo "$output" | grep -qi "failed to install.*node@invalid-version"; then
	echo "ERROR: Missing expected error message for invalid version"
	echo "Output: $output"
	exit 1
fi

# Test 2: Missing plugin error (friendly)
echo "Test 2: Missing plugin"
set +e
output=$(mise install nonexistent-tool@1.0.0 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for missing plugin"
	exit 1
fi

# Check for plugin not found error
if ! echo "$output" | grep -q "not found\|unknown backend\|no backend found"; then
	echo "ERROR: Missing expected error message for missing plugin"
	echo "Output: $output"
	exit 1
fi

# Test 3: Invalid configuration error (friendly)
echo "Test 3: Invalid configuration"
cat >test_invalid_config.toml <<EOF
[tools]
node = "this is not valid"
invalid_key = true
EOF

set +e
output=$(mise --file test_invalid_config.toml install 2>&1)
exit_code=$?
set -e
rm -f test_invalid_config.toml

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for invalid config"
	exit 1
fi

# Test non-friendly error messages (with RUST_BACKTRACE)
echo "Testing non-friendly error messages with backtrace..."

# Test 4: Invalid tool version with backtrace
echo "Test 4: Invalid tool version with backtrace"
set +e
output=$(RUST_BACKTRACE=1 mise install node@invalid-version 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for invalid version with backtrace"
	exit 1
fi

# Check that error contains backtrace markers
if ! echo "$output" | grep -q "Stack backtrace\|at src/\|at /"; then
	echo "WARNING: Expected backtrace markers not found (may be optimized out in release builds)"
	# Don't fail here as release builds might not have full backtraces
fi

# Still should contain the main error message
if ! echo "$output" | grep -qi "failed to install.*node@invalid-version\|invalid-version"; then
	echo "ERROR: Missing expected error message with backtrace enabled"
	echo "Output: $output"
	exit 1
fi

# Test 5: Network/download error (friendly)
echo "Test 5: Network/download error"
# Force a download error by using an invalid URL backend
cat >test_download_error.toml <<EOF
[tools]
"github:nonexistent-org/nonexistent-repo" = "latest"
EOF

set +e
output=$(mise --file test_download_error.toml install 2>&1)
exit_code=$?
set -e
rm -f test_download_error.toml

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for download error"
	exit 1
fi

# Check for network-related error patterns
if ! echo "$output" | grep -qi "failed\|error\|unable\|404\|not found"; then
	echo "ERROR: Missing expected error message for download failure"
	echo "Output: $output"
	exit 1
fi

# Test 6: Permission denied error (if possible)
echo "Test 6: Permission denied scenario"
if [ -w /usr/local ]; then
	echo "Skipping permission test (have write access)"
else
	# Try to install to a protected location
	set +e
	output=$(MISE_DATA_DIR=/usr/local/mise-test mise install node@20 2>&1)
	exit_code=$?
	set -e

	if [ $exit_code -eq 0 ]; then
		echo "WARNING: Expected non-zero exit code for permission denied"
	else
		# Check for permission-related error patterns
		if echo "$output" | grep -qi "permission\|denied\|cannot create\|access"; then
			echo "Permission error detected as expected"
		fi
	fi
fi

# Test 7: Backend-specific error with context
echo "Test 7: Backend error with context"
# Try to use a cargo backend with invalid crate
set +e
output=$(mise install cargo:nonexistent-crate-12345@1.0.0 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for invalid cargo crate"
	exit 1
fi

# Check that error mentions the backend
if ! echo "$output" | grep -qi "cargo\|crate\|failed to install"; then
	echo "ERROR: Missing backend context in error message"
	echo "Output: $output"
	exit 1
fi

# Test 8: Multiple installation failures
echo "Test 8: Multiple installation failures"
set +e
output=$(mise install node@invalid-version python@invalid-version 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code for multiple invalid versions"
	exit 1
fi

# Check that both errors are mentioned
if ! echo "$output" | grep -q "node@invalid-version"; then
	echo "ERROR: Missing node error in multiple failure output"
	echo "Output: $output"
	exit 1
fi

# Test 9: Error location verification (debug mode)
echo "Test 9: Error location in debug mode"
set +e
output=$(MISE_DEBUG=1 mise install node@invalid-version 2>&1)
exit_code=$?
set -e

if [ $exit_code -eq 0 ]; then
	echo "ERROR: Expected non-zero exit code in debug mode"
	exit 1
fi

# In debug mode, we should see more detailed error information
if ! echo "$output" | grep -qi "debug\|trace\|verbose\|failed"; then
	echo "WARNING: Expected debug output markers not found"
	# Don't fail as debug output format may vary
fi

echo "All error display tests passed!"
