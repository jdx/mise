#!/usr/bin/env fish
# shellcheck disable=SC1072,SC1065,SC1064,SC1073,SC2103

# Test that calling mise activate multiple times preserves user-added paths at the front
# while properly handling mise paths (env._.path and tool installs)
#
# Expected behavior:
# - Start: A
# - mise activate: env_path:mise_tools:A
# - add B: B:env_path:mise_tools:A
# - mise activate + hook-env: B:env_path:mise_tools:A (user path B preserved at front)
# - add C: C:B:env_path:mise_tools:A
# - mise activate + hook-env: C:B:env_path:mise_tools:A (user paths C,B preserved at front)
# - deactivate: C:B:A (mise_tools and env_path removed, user paths preserved)

# Create a mise config with a tool, env._.path, and env.FOO
mkdir -p custom_bin
echo >.mise.toml '
[tools]
tiny = "latest"

[env]
_.path = ["./custom_bin"]
FOO = "bar"
BAZ = "qux"
'

mise install
or exit 1

# Save the original PATH for verification
set -l ORIGINAL_PATH $PATH

# First activation
mise activate --status fish | source
__mise_env_eval

# Verify env variables were set
test "$FOO" = "bar"
or begin; echo "FOO should be bar, got: $FOO"; exit 1; end

test "$BAZ" = "qux"
or begin; echo "BAZ should be qux, got: $BAZ"; exit 1; end

# Get the mise tool path - it should be somewhere in PATH
set -l MISE_TOOL_PATH (string join \n $PATH | grep "/mise/installs/tiny" | head -1)
string match -q "*/mise/installs/tiny*" $MISE_TOOL_PATH
or begin; echo "MISE_TOOL_PATH not found"; exit 1; end

# custom_bin from env._.path should be in PATH
string match -q "*custom_bin*" (string join : $PATH)
or begin; echo "custom_bin not in PATH"; exit 1; end

# Verify no duplicate paths
set -l PATH_COUNT (string join \n $PATH | sort | uniq -d | count)
test $PATH_COUNT -eq 0
or begin; echo "Duplicate paths found: $PATH_COUNT"; exit 1; end

# User adds path B
set -x PATH /path_b $PATH

# User modifies FOO
set -x FOO "user_modified"

# Second activation - user path preserved at front, mise paths follow
mise activate --status fish | source
__mise_env_eval

# FOO should be reset to "bar" by mise
test "$FOO" = "bar"
or begin; echo "FOO should be bar after reactivation, got: $FOO"; exit 1; end

# BAZ should still be qux
test "$BAZ" = "qux"
or begin; echo "BAZ should be qux, got: $BAZ"; exit 1; end

# Verify PATH structure: /path_b (user addition) should be first
test "$PATH[1]" = "/path_b"
or begin; echo "First path should be /path_b, got: $PATH[1]"; exit 1; end

string match -q "*custom_bin*" (string join : $PATH)
or begin; echo "custom_bin not in PATH"; exit 1; end

string match -q "*/mise/installs/tiny*" (string join : $PATH)
or begin; echo "Tool path not in PATH"; exit 1; end

# Check no duplicates
set -l PATH_COUNT (string join \n $PATH | sort | uniq -d | count)
test $PATH_COUNT -eq 0
or begin; echo "Duplicate paths found: $PATH_COUNT"; exit 1; end

# User adds path C
set -x PATH /path_c $PATH

# Third activation - user paths preserved at front, mise paths follow
mise activate --status fish | source
__mise_env_eval

# Verify user paths are first (C, B), then mise paths, then original
test "$PATH[1]" = "/path_c"
or begin; echo "First path should be /path_c, got: $PATH[1]"; exit 1; end

string match -q "*/path_b*" (string join : $PATH)
or begin; echo "/path_b not in PATH"; exit 1; end

string match -q "*custom_bin*" (string join : $PATH)
or begin; echo "custom_bin not in PATH"; exit 1; end

string match -q "*/mise/installs/tiny*" (string join : $PATH)
or begin; echo "Tool path not in PATH"; exit 1; end

# Check no duplicates
set -l PATH_COUNT (string join \n $PATH | sort | uniq -d | count)
test $PATH_COUNT -eq 0
or begin; echo "Duplicate paths found: $PATH_COUNT"; exit 1; end

# Test deactivation - unsets mise shell functions and variables
# (Comprehensive deactivation tests are in test_deactivate)
mise deactivate

echo "All tests passed!"
