#!/usr/bin/env bash

set -eu

pkg=$1
cmd=$2
limit=$3
config=$4

opts=""
if [ -n "$cmd" ]; then
	opts="-cmd $cmd"
fi
if [ -n "$limit" ]; then
	opts="$opts -limit $limit"
fi
if [ -n "$config" ] || [ -f "pkgs/$pkg/scaffold.yaml" ]; then
	opts="$opts -c scaffold.yaml"
fi

mkdir -p "pkgs/$pkg"
if [ -n "$config" ];then
	docker cp "$config" "aqua-registry:/workspace/scaffold.yaml"
fi
if [ -z "$config" ] && [ -f "pkgs/$pkg/scaffold.yaml" ]; then
	echo "[INFO] Using pkgs/$pkg/scaffold.yaml" >&2
	docker cp "pkgs/$pkg/scaffold.yaml" "aqua-registry:/workspace/scaffold.yaml"
fi
# shellcheck disable=SC2086
docker exec -ti -w /workspace aqua-registry bash -c "rm pkg.yaml 2>/dev/null || :"
docker exec -ti -w /workspace aqua-registry bash -c "echo '# yaml-language-server: \$schema=https://raw.githubusercontent.com/aquaproj/aqua/main/json-schema/registry.json' > registry.yaml"
docker exec -ti -w /workspace aqua-registry bash -c "aqua gr $opts --out-testdata pkg.yaml \"$pkg\" >> registry.yaml"
docker cp "aqua-registry:/workspace/pkg.yaml" "pkgs/$pkg/pkg.yaml"
docker cp "aqua-registry:/workspace/registry.yaml" "pkgs/$pkg/registry.yaml"
if [ -n "$config" ];then
	cp "$config" "pkgs/$pkg/scaffold.yaml"
fi
