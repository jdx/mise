#!/usr/bin/env bash

set -euo pipefail

# format Shell scripts in scripts/ directory.
shfmt --language-dialect bash --indent 2 --write \
  ./scripts/*

# format Markdown files.
npx -y prettier --write \
  ./**/*.md
