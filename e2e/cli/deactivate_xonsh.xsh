# shellcheck disable=all
$XONSH_SHOW_TRACEBACK = True

# Test that mise deactivate properly works and can be called multiple times safely
# This tests the fix for https://github.com/jdx/mise/issues/6855

from xonsh.built_ins import XSH

# Create a simple mise config
config = """
[tools]
tiny = "latest"

[env]
FOO = "mise_foo"
"""
echo @(config) > .mise.toml

mise install

# Activate mise
# shellcheck disable=SC1073,SC1065,SC1064,SC1072
execx($(mise activate -s xonsh))
events.on_pre_prompt.fire()

# Verify mise is activated
assert 'mise' in aliases, "mise alias should exist after activation"
assert XSH.env.get('FOO') == 'mise_foo', "FOO should be set"

# Deactivate mise
mise deactivate

# Verify mise is deactivated
assert 'mise' not in aliases, "mise alias should be removed after deactivate"

# Test edge case: deactivating when not activated should be safe
# This is the KEY TEST for the fix - using .pop() instead of del
# means this won't raise KeyError
mise deactivate
print("deactivate_twice works - fix verified!")

print("All tests passed!")
