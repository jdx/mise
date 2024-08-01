#!/bin/sh
set -eu

gh release view --json tagName | jq -r '.tagName'
