#!/usr/bin/env bash
set -euxo pipefail

# This script runs only after the GitHub release is public. It updates the
# mutable latest files, including VERSION, while publishing the release to R2.

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
