#!/usr/bin/env bash
#MISE alias=["format", "fix"]
#MISE wait_for=["render:settings"]
set -euxo pipefail

markdownlint --fix .
taplo fmt
SHELLCHECK_OPTS="--exclude=SC1090 --exclude=SC2046" actionlint
toml-sort -i settings.toml --spaces-indent-inline-array 4
toml-sort -i registry.toml --spaces-indent-inline-array 4

cat >rustfmt.toml <<EOF
unstable_features = true
imports_granularity = "Module"
EOF
cargo fmt --all
rm rustfmt.toml

hk fix --all
