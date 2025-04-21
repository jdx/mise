#!/bin/bash
set -e

source dev-container-features-test-lib

check "shims disabled" bash -c 'mise doctor | grep "shims_on_path: no"'

reportResults
