#!/usr/bin/env fish
# shellcheck disable=SC1072,SC1065,SC1064,SC1073,SC2103

# Test that mise deactivate properly removes mise-managed environment
# while preserving user-added paths and variables

# Create a mise config with tools, env._.path, and env vars
mkdir -p custom_bin shared_entry user_bin
set -l SHARED_PATH "$PWD/shared_entry"
echo >.mise.toml '
[tools]
tiny = "latest"

[env]
_.path = ["./custom_bin", "./shared_entry"]
FOO = "mise_foo"
BAR = "mise_bar"
'

mise install
or exit 1

# Save the original PATH and environment
set -l ORIGINAL_PATH $PATH

# Activate mise and run hook-env
mise activate --status fish | source
__mise_env_eval

# Verify mise environment is set up
test "$FOO" = "mise_foo"
or begin; echo "FOO should be mise_foo, got: $FOO"; exit 1; end

test "$BAR" = "mise_bar"
or begin; echo "BAR should be mise_bar, got: $BAR"; exit 1; end

string match -q "*custom_bin*" (string join : $PATH)
or begin; echo "custom_bin not in PATH"; exit 1; end

string match -q "*/mise/installs/tiny*" (string join : $PATH)
or begin; echo "Tool path not in PATH"; exit 1; end

string match -q "*$SHARED_PATH*" (string join : $PATH)
or begin; echo "SHARED_PATH not in PATH"; exit 1; end

# User manually adds paths after activation
set -x PATH $SHARED_PATH $PATH
set -x PATH /user_path_1 $PATH
set -x PATH /user_path_2 $PATH
# Add SHARED_PATH again after mise so PATH now has it before, within, and after
set -x PATH $PATH $SHARED_PATH

# User sets their own environment variables
set -x USER_VAR "user_value"
set -x USER_PATH "/usr/local/myapp"

# Verify everything is in place before deactivation
test "$FOO" = "mise_foo"
or begin; echo "FOO should be mise_foo, got: $FOO"; exit 1; end

test "$USER_VAR" = "user_value"
or begin; echo "USER_VAR should be user_value, got: $USER_VAR"; exit 1; end

string match -q "*/user_path_1*" (string join : $PATH)
or begin; echo "/user_path_1 not in PATH"; exit 1; end

string match -q "*/user_path_2*" (string join : $PATH)
or begin; echo "/user_path_2 not in PATH"; exit 1; end

string match -q "*custom_bin*" (string join : $PATH)
or begin; echo "custom_bin not in PATH"; exit 1; end

string match -q "*/mise/installs/tiny*" (string join : $PATH)
or begin; echo "Tool path not in PATH"; exit 1; end

string match -q "*$SHARED_PATH*" (string join : $PATH)
or begin; echo "SHARED_PATH not in PATH"; exit 1; end

# SHARED_PATH should appear three times (before, inside, and after mise entries)
set -l SHARED_COUNT (string join \n $PATH | grep -Fxc "$SHARED_PATH")
test $SHARED_COUNT -eq 3
or begin; echo "SHARED_PATH should appear 3 times, got: $SHARED_COUNT"; exit 1; end

# Deactivate mise
mise deactivate

# After deactivation:

# 1. Mise-managed environment variables should be removed
set -q FOO
and begin; echo "FOO should be unset, got: $FOO"; exit 1; end

set -q BAR
and begin; echo "BAR should be unset, got: $BAR"; exit 1; end

# 2. User's own environment variables should be preserved
test "$USER_VAR" = "user_value"
or begin; echo "USER_VAR should be user_value, got: $USER_VAR"; exit 1; end

test "$USER_PATH" = "/usr/local/myapp"
or begin; echo "USER_PATH should be /usr/local/myapp, got: $USER_PATH"; exit 1; end

# 3. Mise-managed paths should be removed from PATH
if string match -q "*/mise/installs/tiny*" (string join : $PATH)
    echo "Tool path should not be in PATH after deactivation"
    exit 1
end

if string match -q "*custom_bin*" (string join : $PATH)
    echo "custom_bin should not be in PATH after deactivation"
    exit 1
end

# Shared path should now appear exactly twice (before and after mise)
set -l SHARED_COUNT (string join \n $PATH | grep -Fxc "$SHARED_PATH")
test $SHARED_COUNT -eq 2
or begin; echo "SHARED_PATH should appear 2 times after deactivation, got: $SHARED_COUNT"; exit 1; end

# 4. User-added paths should be preserved
string match -q "*/user_path_1*" (string join : $PATH)
or begin; echo "/user_path_1 should be preserved"; exit 1; end

string match -q "*/user_path_2*" (string join : $PATH)
or begin; echo "/user_path_2 should be preserved"; exit 1; end

# 5. Original PATH components should still be present
string match -q "*$ORIGINAL_PATH[1]*" (string join : $PATH)
or begin; echo "Original PATH components should be preserved"; exit 1; end

# 6. No duplicate paths other than SHARED_PATH
set -l PATH_COUNT (string join \n $PATH | grep -Fvx "$SHARED_PATH" | sort | uniq -d | count)
test $PATH_COUNT -eq 0
or begin; echo "No duplicate paths expected other than SHARED_PATH, got: $PATH_COUNT"; exit 1; end

# 7. Mise shell integration should be removed
# The mise function should no longer exist
if type -q __mise_env_eval
    echo "ERROR: __mise_env_eval function still exists after deactivate"
    exit 1
end

# 8. Test edge case: deactivating when not activated should be safe
mise deactivate
echo "deactivate_twice works"

# 9. Test reactivation after deactivation
mise activate --status fish | source
__mise_env_eval

# After reactivation, mise env vars should be back
test "$FOO" = "mise_foo"
or begin; echo "FOO should be mise_foo after reactivation, got: $FOO"; exit 1; end

test "$BAR" = "mise_bar"
or begin; echo "BAR should be mise_bar after reactivation, got: $BAR"; exit 1; end

# User additions should still be preserved
string match -q "*/user_path_1*" (string join : $PATH)
or begin; echo "/user_path_1 should be preserved"; exit 1; end

string match -q "*/user_path_2*" (string join : $PATH)
or begin; echo "/user_path_2 should be preserved"; exit 1; end

string match -q "*$SHARED_PATH*" (string join : $PATH)
or begin; echo "SHARED_PATH should be preserved"; exit 1; end

# After reactivation, shared_entry appears 3 times:
# - Once from env._.path config (mise adds it to maintain user's intended ordering)
# - Twice from user manual additions (preserved in PATH)
# This is correct behavior: user-configured paths are always added even if they
# exist elsewhere in PATH, to ensure the user's intended precedence is maintained.
set -l SHARED_COUNT (string join \n $PATH | grep -Fxc "$SHARED_PATH")
test $SHARED_COUNT -eq 3
or begin; echo "SHARED_PATH should appear 3 times after reactivation, got: $SHARED_COUNT"; exit 1; end

# And user vars should still be there
test "$USER_VAR" = "user_value"
or begin; echo "USER_VAR should be user_value, got: $USER_VAR"; exit 1; end

# 10. Test deactivation with modified env vars
# If user modifies a mise-managed variable, deactivation should still remove it
set -x FOO "user_modified_foo"
mise deactivate

# FOO should be removed entirely by deactivation (mise doesn't track user modifications)
set -q FOO
and begin; echo "FOO should be unset after deactivation, got: $FOO"; exit 1; end

echo "All tests passed!"
