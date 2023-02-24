#!/usr/bin/env bash
set -euxo pipefail

./scripts/update-shorthand-repo.sh
just lint-fix
git add src/default_shorthands.rs
