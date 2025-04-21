#!/bin/bash
set -e

source dev-container-features-test-lib

check "no activation" bash -c 'mise doctor | grep "activated: no"'

reportResults
