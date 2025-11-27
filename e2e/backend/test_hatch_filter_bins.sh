#!/usr/bin/env bash

# Test filter_bins option for github backend with hatch, specifically ensuring Python is not exposed.

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Skipping Linux-specific test on non-Linux OS"
  exit 0
fi

# Set MISE_EXPERIMENTAL for github backend features
export MISE_EXPERIMENTAL=1

# Install a known python version globally
mise use --global python@3.11 || mise install python@3.11 && mise use --global python@3.11

# Install hatch (should use github backend from registry)
mise install hatch@latest

# Get the base install path for hatch
hatch_install_dir="$(mise where hatch@latest)"

# Verify hatch binary is in .mise-bins
hatch_bin_dir="$hatch_install_dir/.mise-bins"
assert_directory_exists "$hatch_bin_dir"
if [[ -f "$hatch_bin_dir/hatch" ]]; then
    echo "hatch binary exists in .mise-bins"
else
    echo "hatch binary missing from .mise-bins"
    exit 1
fi

# Verify no python executable is exposed via hatch's .mise-bins
# The filter_bins option should ensure only 'hatch' is linked.
# Therefore, 'python' or 'python3' should not be found in the .mise-bins directory.
if find "$hatch_bin_dir" -maxdepth 1 -name "python*" | grep -q "python"; then
    echo "ERROR: Python executable found in hatch's .mise-bins directory. Filter_bins is not working."
    exit 1
fi
echo "Verified no unexpected python executables from hatch's .mise-bins."

# Verify hatch itself works
assert_contains "mise x hatch@latest -- hatch --version" "Hatch"

# Verify global python is still the one mise manages and not hatch's bundled one
assert_contains "mise x python -- python --version" "Python 3.11"

# Clean up
mise uninstall hatch@latest
mise uninstall python@3.11
