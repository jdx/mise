#!/usr/bin/env bash

set -euo pipefail

# check format Shell scripts in scripts/ directory.
shfmt --language-dialect bash --indent 2 --diff \
  ./scripts/*

# check format Markdown files.
npx -y prettier --check \
  ./**/*.md

# lint for errors in Shell scripts in scripts/ directory.
shellcheck --shell bash --external-sources \
  ./scripts/*
