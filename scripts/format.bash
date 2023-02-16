#!/usr/bin/env bash

set -euo pipefail

printf "* %s\n\n" "Formatting shell scripts..."

# format Shell scripts in scripts/ directory.
shfmt --language-dialect bash --find \
  ./scripts/*
shfmt --language-dialect bash --indent 2 --write \
  ./scripts/*

printf "\n* %s\n\n" "Formatting markdown..."

# format Markdown files.
npx -y prettier --write \
  ./**/*.md

printf "\n* %s\n" "Formatting complete!"
