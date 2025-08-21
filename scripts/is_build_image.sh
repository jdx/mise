#!/usr/bin/env bash

set -eu

if ! docker inspect aquaproj/aqua-registry >/dev/null; then
	# image doesn't exist
	exit 1
fi

if [ ! -f .build/Dockerfile ]; then
	exit 1
fi

diff -q docker/Dockerfile .build/Dockerfile
