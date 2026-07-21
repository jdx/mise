#!/usr/bin/env bash
set -euxo pipefail

# This script runs after the GitHub release is successfully created
# It updates the VERSION file and publishes to R2

BASE_DIR="$(pwd)"
MISE_VERSION=$(./scripts/get-version.sh)
RELEASE_DIR=releases
export BASE_DIR MISE_VERSION RELEASE_DIR

echo "::group::Create VERSION file"
pushd "$RELEASE_DIR"
echo "$MISE_VERSION" | tr -d 'v' >VERSION
popd

if [[ ${DRY_RUN:-0} != 1 ]]; then
	echo "::group::Publish r2"
	./scripts/publish-r2.sh
fi
