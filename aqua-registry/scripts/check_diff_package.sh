#!/usr/bin/env bash

set -eu

if ! git diff --quiet pkgs registry.yaml; then
    echo "[ERROR] The directory pkgs or registry.yaml has changes." >&2
    git diff --name-only --exit-code pkgs registry.yaml
fi

if ! git diff --cached --quiet pkgs registry.yaml; then
    echo "[ERROR] The directory pkgs or registry.yaml has changes." >&2
    git diff --cached --name-only --exit-code pkgs registry.yaml
fi

if [ -n "$(git ls-files --others --exclude-standard pkgs)" ]; then
    echo "[ERROR] The directory pkgs or registry.yaml has changes." >&2
    git ls-files --others --exclude-standard pkgs
    exit 1
fi
