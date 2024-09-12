#!/bin/sh
set -eu

gh release view -R jdx/mise --json tagName | jq -r '.tagName'
