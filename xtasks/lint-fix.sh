#!/usr/bin/env bash
#MISE alias=["format", "fix"]
#MISE wait_for=["render:schema"]
#MISE description="Automatically fix lint issues"
set -euxo pipefail

hk fix --all --exclude vendor/aqua-registry
