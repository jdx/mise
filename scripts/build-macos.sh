#!/usr/bin/env bash
set -euxo pipefail

TARGET="$1"

scripts/build-tarball.sh rtx --release --features self_update --target "$TARGET"
scripts/build-tarball.sh rtx-brew --release --features brew --target "$TARGET"
