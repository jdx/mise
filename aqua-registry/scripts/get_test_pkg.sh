#!/usr/bin/env bash

set -euo pipefail

pkg=${1:-} 

if [ -n "$pkg" ]; then
    echo "$pkg"
    exit 0
fi

current_branch=$(git branch | grep "^\* " | sed -e "s/^\* \(.*\)/\1/")

if ! [[  $current_branch =~ ^feat/ ]]; then
    echo "[ERROR] The current branch name must be feat/<package name> or you must give a package name" >&2
    exit 1
fi

echo ${current_branch#feat/}
