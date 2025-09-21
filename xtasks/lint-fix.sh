#!/usr/bin/env bash
#MISE alias=["format", "fix"]
#MISE wait_for=["render:schema"]
#MISE description="Automatically fix lint issues"
set -euxo pipefail

markdownlint --fix .
SHELLCHECK_OPTS="--exclude=SC1090 --exclude=SC2046 --exclude=SC2086 --exclude=SC2129" actionlint

cat >rustfmt.toml <<EOF
unstable_features = true
imports_granularity = "Module"
EOF
cargo fmt --all
rm rustfmt.toml

hk fix --all --exclude crates/aqua-registry/aqua-registry
