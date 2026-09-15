#!/usr/bin/env bash
set -euxo pipefail

# This script stages release assets in R2 while the GitHub release is still a
# draft. The VERSION pointer is published separately after the release becomes
# public.

BASE_DIR="$(pwd)"
MISE_VERSION=$(./scripts/get-version.sh)
RELEASE_DIR=releases
export BASE_DIR MISE_VERSION RELEASE_DIR

if [[ ${DRY_RUN:-0} != 1 ]]; then
	echo "::group::Publish r2"
	./scripts/publish-r2.sh
fi
