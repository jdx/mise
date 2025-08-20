#!/usr/bin/env bash

set -euo pipefail

pkg=$1

docker cp "pkgs/$pkg/pkg.yaml" "aqua-registry-windows:/workspace/pkg.yaml"
docker cp "pkgs/$pkg/registry.yaml" "aqua-registry-windows:/workspace/registry.yaml"

for arch in amd64 arm64; do
	if ! docker exec aqua-registry-windows env AQUA_GOOS="windows" AQUA_GOARCH="$arch" aqua i; then
		echo "[ERROR] Build failed windows/$arch" >&2
		echo "        If you want to look into the container, please run 'cmdx con windows $arch'" >&2
		exit 1
	fi
done
