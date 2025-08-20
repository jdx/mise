#!/usr/bin/env bash

set -eu

if bash scripts/exist_container.sh; then
	docker stop -t 1 aqua-registry
	docker rm aqua-registry
fi

if bash scripts/exist_container.sh aqua-registry-windows; then
	docker stop -t 1 aqua-registry-windows
	docker rm aqua-registry-windows
fi
