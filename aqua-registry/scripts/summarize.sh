#!/usr/bin/env bash

set -euo pipefail

pr=$1

env=$(ci-info run -owner aquaproj -repo aqua-registry -pr "$pr")
eval "$env"

list_pkgs() {
    grep -E "^pkgs/.*\.yaml" "$CI_INFO_TEMP_DIR/pr_files.txt" | sed -E "s|^pkgs/(.+)/[^/]+\.yaml|\1|" | sort -u
}

while read -r pkg; do
    desc=$(yq ".packages[0].description" "pkgs/$pkg/registry.yaml")
    repo_owner=$(yq ".packages[0].repo_owner" "pkgs/$pkg/registry.yaml")
    repo_name=$(yq ".packages[0].repo_name" "pkgs/$pkg/registry.yaml")
    if [ "$repo_owner" == "null" ] || [ "$repo_name" == "null" ]; then
        echo "$pkg - $desc"
        continue
    fi
    echo "[$pkg](https://github.com/$repo_owner/$repo_name) - $desc"
done < <(list_pkgs)
