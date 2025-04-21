#!/bin/bash
set -e

source dev-container-features-test-lib

stat -c '%U' $(command -v mise)
check "owner" bash -c '[ "$(stat -c '%U' $(command -v mise))" = "mise" ]'

# Non-root user can do self-update
## TODO: The self-replace crate tries to create the /usr/local/bin/.mise.tmp file, but it results in a "Permission denied" error.
## https://github.com/mitsuhiko/self-replace/blob/8365c59b29157191e8b60022e9fe0b886affdc0d/src/unix.rs#L28
#check "version before update" bash -c 'mise version --json | [ "$(jq -r .version)" = "2025.4.1 linux-x64 (2025-04-09)" ]'
#mise self-update v2025.4.2 --yes
#check "version after update" bash -c 'mise version --json | [ "$(jq -r .version)" = "2025.4.2 linux-x64 (2025-04-09)" ]'

reportResults
