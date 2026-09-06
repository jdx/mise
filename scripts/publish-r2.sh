#!/usr/bin/env bash
set -euxo pipefail

export AWS_REGION=auto
export AWS_DEFAULT_OUTPUT=json
export AWS_ENDPOINT_URL=https://6e243906ff257b965bcae8025c2fc344.r2.cloudflarestorage.com
export AWS_ACCESS_KEY_ID=$CLOUDFLARE_ACCESS_KEY_ID
export AWS_SECRET_ACCESS_KEY=$CLOUDFLARE_SECRET_ACCESS_KEY
export AWS_S3_BUCKET=mise

./scripts/publish-s3.sh
