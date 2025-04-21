#!/bin/bash
set -e

source dev-container-features-test-lib

check "shims enabled" bash -c 'mise doctor | grep "shims_on_path: yes"'

reportResults
