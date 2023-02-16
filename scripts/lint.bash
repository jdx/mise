#!/usr/bin/env bash

set -euo pipefail

# check format Markdown files.
npx -y prettier --check \
  ./**/*.md

printf "* %s\n" "Linting shell scripts..."

# check format Shell scripts in scripts/ directory.
shfmt --language-dialect bash --indent 2 --diff \
  ./scripts/*

# lint for errors in Shell scripts in scripts/ directory.
shellcheck --shell bash --external-sources \
  ./scripts/*

printf "* %s\n" "All matched using shellcheck & shfmt!"
