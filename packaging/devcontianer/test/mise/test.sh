#!/bin/bash

# This test file will be executed against an auto-generated devcontainer.json that
# includes the 'mise' Feature with no options.
#
# For more information, see: https://github.com/devcontainers/cli/blob/main/docs/features/test.md
#
# Eg:
# {
#    "image": "<..some-base-image...>",
#    "features": {
#      "mise": {}
#    },
#    "remoteUser": "mise"
# }

set -e

# Optional: Import test library bundled with the devcontainer CLI
# See https://github.com/devcontainers/cli/blob/HEAD/docs/features/test.md#dev-container-features-test-lib
# Provides the 'check' and 'reportResults' commands.
source dev-container-features-test-lib

# Feature-specific tests
# The 'check' command comes from the dev-container-features-test-lib. Syntax is...
# check <LABEL> <cmd> [args...]
check "execute command" bash -c "mise docker | grep 'No problems found'"

# Report results
# If any of the checks above exited with a non-zero exit code, the test will fail.
reportResults
