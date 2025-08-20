#!/usr/bin/env bash

set -eu

if ! git diff --quiet pkgs; then
    echo "[ERROR] The directory pkgs has changes" >&2
    git diff --name-only --exit-code pkgs
fi

if ! git diff --cached --quiet pkgs; then
    echo "[ERROR] The directory pkgs has changes" >&2
    git diff --cached --name-only --exit-code pkgs
fi

if [ -n "$(git ls-files --others --exclude-standard pkgs)" ]; then
    echo "[ERROR] The directory pkgs has changes" >&2
    git ls-files --others --exclude-standard pkgs
    exit 1
fi
