#!/usr/bin/env bash

set -euo pipefail

container_name=${1:-aqua-registry}

echo "[INFO] Checking if the container $container_name is running" >&2
docker ps -a \
	--filter "name=$container_name" \
	--filter status=running \
	--format "{{.Names}}" |
	grep -E "^$container_name$" >/dev/null
