#!/usr/bin/env bash

set -eu

cp aqua-policy.yaml docker
docker build -t aquaproj/aqua-registry docker
mkdir -p .build
cp docker/Dockerfile .build
