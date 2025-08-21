#!/usr/bin/env bash

set -euo pipefail

container_name=${1:-aqua-registry}

if ! bash scripts/is_build_image.sh; then
	echo "[INFO] Building the docker image aquaproj/aqua-registry" >&2
	bash scripts/build_image.sh
fi

if ! bash scripts/exist_container.sh "$container_name"; then
	echo "[INFO] Creating a container $container_name" >&2
	bash scripts/run.sh "$container_name"
	exit 0
fi

if bash scripts/is_container_running.sh "$container_name"; then
	if bash scripts/check_image.sh "$container_name"; then
		echo "[INFO] Dockerfile isn't updated" >&2
		exit 0
	fi
	echo "[INFO] Dockerfile is updated, so the container $container_name is being recreated" >&2
	bash scripts/remove_container.sh "$container_name"
	bash scripts/run.sh "$container_name"
	exit 0
fi

if bash scripts/check_image.sh "$container_name"; then
	echo "[INFO] Dockerfile isn't updated" >&2
	echo "[INFO] Starting the container $container_name" >&2
	docker start "$container_name"
	exit 0
fi

echo "[INFO] Dockerfile is updated, so the container $container_name is being recreated" >&2
bash scripts/remove_container.sh "$container_name"
bash scripts/run.sh "$container_name"
