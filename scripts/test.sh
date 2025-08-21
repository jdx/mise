#!/usr/bin/env bash

set -eu

pkg=$1
container_name=aqua-registry

docker cp "pkgs/$pkg/pkg.yaml" "$container_name:/workspace/pkg.yaml"
docker cp "pkgs/$pkg/registry.yaml" "$container_name:/workspace/registry.yaml"

for os in linux darwin; do
	for arch in amd64 arm64; do
		docker exec "$container_name" bash -c "rm aqua-checksums.json 2>/dev/null || :"
		if ! docker exec "$container_name" env AQUA_GOOS="$os" AQUA_GOARCH="$arch" aqua i; then
			echo "[ERROR] Build failed $os/$arch" >&2
			echo "        If you want to look into the container, please run 'cmdx con $os $arch'" >&2
			exit 1
		fi
	done
done

aqua exec -- aqua-registry gr
