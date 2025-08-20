#!/usr/bin/env bash

set -eu

# Disable the check https://github.com/suzuki-shunsuke/oss-contribution-guide/issues/77

# if [ "$(git config commit.gpgSign)" != true ]; then
#     echo "[ERROR] This repository requires commit signing, so please configure it." >&2
#     echo "        https://github.com/suzuki-shunsuke/oss-contribution-guide/blob/main/docs/commit-signing.md" >&2
#     exit 1
# fi
