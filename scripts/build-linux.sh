#!/usr/bin/env bash
set -euxo pipefail

TARGET="$1"
export CROSS=1

scripts/build-tarball.sh rtx --release --features self_update --target "$TARGET"
scripts/build-tarball.sh rtx-brew --release --features brew --target "$TARGET"
scripts/build-tarball.sh rtx-deb --release --features deb --target "$TARGET"
scripts/build-tarball.sh rtx-rpm --release --features rpm --target "$TARGET"
