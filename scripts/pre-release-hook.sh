#!/usr/bin/env bash
set -euxo pipefail

just render-mangen render-help

./scripts/update-shorthand-repo.sh
just lint-fix

git add man src/default_shorthands.rs
