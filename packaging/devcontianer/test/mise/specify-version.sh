#!/bin/bash
set -e

source dev-container-features-test-lib

check "version" bash -c 'mise version --json | [ "$(jq -r .version)" = "2025.4.1 linux-x64 (2025-04-09)" ]'

reportResults
