#!/usr/bin/env bash
set -euxo pipefail

if [[ "${NO_UPDATE:-}" == "1" ]]; then
	echo "NO_UPDATE is set, skipping update"
else
	cargo update && git add Cargo.lock
fi

just render-mangen render-help

./scripts/update-shorthand-repo.sh
just lint-fix
just build-core-plugins

git add man src/default_shorthands.rs
