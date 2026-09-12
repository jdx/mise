#!/bin/sh
set -eu

echo "v$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f 2)"
