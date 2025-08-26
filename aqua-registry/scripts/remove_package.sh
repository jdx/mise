#!/usr/bin/env bash

set -euo pipefail

pkg=$1

remove() {
    container=$1
    docker exec "$container" aqua rm "$pkg"
    docker exec "$container" bash -c "! test -f aqua-checksums.json || rm aqua-checksums.json"
}

remove aqua-registry
remove aqua-registry-windows
