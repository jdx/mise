#!/usr/bin/env bash

set -euo pipefail

current_branch=$(git branch | grep "^\* " | sed -e "s/^\* \(.*\)/\1/")

pkg=${1:-} 

if [ -n "$pkg" ] && [ "$current_branch" != "feat/$pkg" ]; then
    echo "[ERROR] The current branch must be feat/$pkg" >&2
    exit 1
fi

if ! [[  $current_branch =~ ^feat/ ]]; then
    echo "[ERROR] The branch name must be feat/<package name>" >&2
    exit 1
fi

if [ -n "$pkg" ]; then
    echo "$pkg"
    exit 0
fi

echo ${current_branch#feat/}
