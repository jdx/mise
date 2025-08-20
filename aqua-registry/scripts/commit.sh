#!/usr/bin/env bash

set -eu

pkg=$1

git add registry.yaml pkgs/$pkg/*.yaml
git commit -m "feat($pkg): scaffold $pkg"
