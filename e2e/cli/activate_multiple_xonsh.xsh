# shellcheck disable=all
$XONSH_SHOW_TRACEBACK = True

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

from xonsh.built_ins import XSH
import os

# Create a mise config with a tool, env._.path, and env.FOO
mkdir -p custom_bin
config = """
[tools]
tiny = "latest"

[env]
_.path = ["./custom_bin"]
FOO = "bar"
BAZ = "qux"
"""
echo @(config) > .mise.toml

mise install

# Save the original PATH for verification
ORIGINAL_PATH = list(XSH.env['PATH'])

# First activation
# shellcheck disable=SC1073,SC1065,SC1064,SC1072
execx($(mise activate -s xonsh))

# Fire the hook to apply environment
events.on_pre_prompt.fire()

# Verify env variables were set
assert XSH.env.get('FOO') == 'bar', f"FOO should be bar, got: {XSH.env.get('FOO')}"
assert XSH.env.get('BAZ') == 'qux', f"BAZ should be qux, got: {XSH.env.get('BAZ')}"

# Check that mise was activated at all
assert 'mise' in aliases

# custom_bin from env._.path should be in PATH
current_path = ':'.join(XSH.env['PATH'])
assert 'custom_bin' in current_path, "custom_bin not in PATH"

# Get the mise tool path - it should be somewhere in PATH
mise_tool_path = None
for p in XSH.env['PATH']:
    if '/mise/installs/tiny' in p:
        mise_tool_path = p
        break
assert mise_tool_path is not None, "MISE_TOOL_PATH not found"

# Verify no duplicate paths
path_list = XSH.env['PATH']
assert len(path_list) == len(set(path_list)), "Duplicate paths found"

# User adds path B
XSH.env['PATH'].add('/path_b', front=True)

# User modifies FOO
XSH.env['FOO'] = 'user_modified'

# Second activation - user path preserved at front, mise paths follow
# This tests that activation is idempotent when __MISE_DIFF exists but mise isn't in aliases
# (simulating nested shells like tmux)
del aliases['mise']
# shellcheck disable=SC1073,SC1065,SC1064,SC1072
execx($(mise activate -s xonsh))
events.on_pre_prompt.fire()

# FOO should be reset to "bar" by mise
assert XSH.env.get('FOO') == 'bar', f"FOO should be bar after reactivation, got: {XSH.env.get('FOO')}"

# BAZ should still be qux
assert XSH.env.get('BAZ') == 'qux', f"BAZ should be qux, got: {XSH.env.get('BAZ')}"

# Verify mise paths are in PATH
current_path = ':'.join(XSH.env['PATH'])
assert '/path_b' in current_path, "/path_b not in PATH after second activation"
assert 'custom_bin' in current_path, "custom_bin not in PATH after second activation"
assert '/mise/installs/tiny' in current_path, "Tool path not in PATH after second activation"

# Check no duplicates
path_list = XSH.env['PATH']
assert len(path_list) == len(set(path_list)), "Duplicate paths found after second activation"

# User adds path C
XSH.env['PATH'].add('/path_c', front=True)

# Third activation - user paths preserved at front, mise paths follow
# shellcheck disable=SC1073,SC1065,SC1064,SC1072
execx($(mise activate -s xonsh))
events.on_pre_prompt.fire()

# Verify all paths are present
current_path = ':'.join(XSH.env['PATH'])
assert '/path_c' in current_path, "/path_c not in PATH"
assert '/path_b' in current_path, "/path_b not in PATH"
assert 'custom_bin' in current_path, "custom_bin not in PATH"
assert '/mise/installs/tiny' in current_path, "Tool path not in PATH"

# Check no duplicates
path_list = XSH.env['PATH']
assert len(path_list) == len(set(path_list)), "Duplicate paths found after third activation"

# Test deactivation - unsets mise shell functions and variables
# (Comprehensive deactivation tests are in test_deactivate)
mise deactivate

print("All tests passed!")
