#!/usr/bin/env bash
#MISE description="Render JSON schema"
set -euo pipefail

# mise-config generates schema/mise.json, schema/mise-task.json and schema/miserc.json from its
# types and settings.toml. Like CI's other cargo commands, this skips the MSRV check: the lint
# runner's rustc can be older than the MSRV.
cargo run --quiet --ignore-rust-version --package mise-config --example render-schema
oxfmt --write schema/mise.json schema/mise-task.json schema/miserc.json
