#!/usr/bin/env bash

set -euo pipefail

container_name=${1:-aqua-registry}

container_image_id=$(docker inspect "$container_name" | aqua exec -- jq -r ".[].Image")
image_id=$(docker inspect aquaproj/aqua-registry | aqua exec -- jq -r ".[].Id")

[ "$container_image_id" = "$image_id" ]
