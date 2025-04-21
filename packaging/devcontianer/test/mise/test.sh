#!/bin/bash
set -e

source dev-container-features-test-lib

check "no problems in doctor" bash -c "mise doctor | grep 'No problems found'"

reportResults
