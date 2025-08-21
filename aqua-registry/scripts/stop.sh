#!/usr/bin/env bash

set -eu

container=${1:-aqua-registry}

if bash scripts/exist_container.sh "$container"; then
	docker stop -t 1 "$container"
fi
