#!/usr/bin/env bash

set -eu

PACKAGE=${PACKAGE#https://github.com/}

bash scripts/check_diff_package.sh
pkg=$(bash scripts/get_pkg_from_branch.sh "$PACKAGE")
aqua exec -- aqua-registry create-pr-new-pkg "$pkg"
