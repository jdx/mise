#!/usr/bin/env bash

set -eu

if [ "$NO_CREATE_BRANCH" = true ]; then
    exit 0
fi

pkg=$1
branch=feat/$pkg

if git show-ref --quiet "refs/heads/$branch"; then
    git checkout "$branch"
    exit 0
fi

temp_remote="temp-remote-$(date +%Y%m%d%H%M%S)"

git remote add "$temp_remote" https://github.com/aquaproj/aqua-registry
git fetch "$temp_remote" main
git checkout -b "feat/$pkg" "$temp_remote/main"
git remote remove "$temp_remote"
