#!/usr/bin/env bash
set -euxo pipefail

#cache_hour="max-age=3600,s-maxage=3600,public,immutable"
cache_day="max-age=86400,s-maxage=86400,public,immutable"
cache_week="max-age=604800,s-maxage=604800,public,immutable"

aws s3 cp "$RELEASE_DIR/$MISE_VERSION" "s3://$AWS_S3_BUCKET/$MISE_VERSION/" --cache-control "$cache_week" --no-progress --recursive --include "*" --exclude "*.tar.gz" --exclude "*.tar.xz"

aws s3 cp "$RELEASE_DIR" "s3://$AWS_S3_BUCKET/" --cache-control "$cache_day" --no-progress --recursive --exclude "*.tar.gz" --exclude "*.tar.xz" --include "mise-latest-*"
aws s3 cp "$RELEASE_DIR" "s3://$AWS_S3_BUCKET/" --cache-control "$cache_day" --no-progress --content-type "text/plain" --recursive --exclude "*" --include "SHASUMS*"
aws s3 cp "$RELEASE_DIR/VERSION" "s3://$AWS_S3_BUCKET/" --cache-control "$cache_day" --no-progress --content-type "text/plain"
aws s3 cp "$RELEASE_DIR/install.sh" "s3://$AWS_S3_BUCKET/" --cache-control "$cache_day" --no-progress --content-type "text/plain"
aws s3 cp "$RELEASE_DIR/install.sh.sig" "s3://$AWS_S3_BUCKET/" --cache-control "$cache_day" --no-progress
aws s3 cp "./mise/schema/mise.json" "s3://$AWS_S3_BUCKET/schema/mise.json" --cache-control "$cache_day" --no-progress --content-type "application/json"
aws s3 cp "./mise/schema/settings.json" "s3://$AWS_S3_BUCKET/schema/settings.json" --cache-control "$cache_day" --no-progress --content-type "application/json"
aws s3 cp "./mise/schema/mise.plugin.json" "s3://$AWS_S3_BUCKET/schema/mise.plugin.json" --cache-control "$cache_day" --no-progress --content-type "application/json"

aws s3 cp artifacts/rpm/mise.repo "s3://$AWS_S3_BUCKET/rpm/" --cache-control "$cache_day" --no-progress
aws s3 cp artifacts/rpm/packages/ "s3://$AWS_S3_BUCKET/rpm/packages/" --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/rpm/repodata/ "s3://$AWS_S3_BUCKET/rpm/repodata/" --cache-control "$cache_day" --no-progress --recursive --exclude "*" --include "repomd.xml*"
aws s3 cp artifacts/rpm/repodata/ "s3://$AWS_S3_BUCKET/rpm/repodata/" --cache-control "$cache_week" --no-progress --recursive --exclude "repomd.xml*"

aws s3 cp artifacts/deb/pool/ "s3://$AWS_S3_BUCKET/deb/pool/" --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/deb/dists/ "s3://$AWS_S3_BUCKET/deb/dists/" --cache-control "$cache_day" --no-progress --no-progress --recursive

export CLOUDFLARE_ACCOUNT_ID=6e243906ff257b965bcae8025c2fc344

# jdx.dev
curl --fail-with-body -X POST "https://api.cloudflare.com/client/v4/zones/90dfd7997bdcfa8579c52d8ee8dd4cd1/purge_cache" \
  -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{ "purge_everything": true }'

# rtx.pub
curl --fail-with-body -X POST "https://api.cloudflare.com/client/v4/zones/80d977fd09f01db52bec165778088891/purge_cache" \
  -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{ "purge_everything": true }'

# mise.run
curl --fail-with-body -X POST "https://api.cloudflare.com/client/v4/zones/782fc08181b7bbd26c529a00df52a277/purge_cache" \
  -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  -H "Content-Type: application/json" \
  --data '{ "purge_everything": true }'
