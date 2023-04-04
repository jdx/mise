#!/usr/bin/env bash
set -euxo pipefail

#cache_hour="max-age=3600,s-maxage=3600,public,immutable"
cache_day="max-age=86400,s-maxage=86400,public,immutable"
cache_week="max-age=604800,s-maxage=604800,public,immutable"

./rtx/scripts/render-install.sh >"$RELEASE_DIR"/install.sh
echo "$RTX_VERSION" | tr -d 'v' >"$RELEASE_DIR"/VERSION

cp "$RELEASE_DIR/rtx-latest-linux-x64" "$RELEASE_DIR/rtx-latest-linux-amd64"
cp "$RELEASE_DIR/rtx-latest-macos-x64" "$RELEASE_DIR/rtx-latest-macos-amd64"

aws s3 cp "$RELEASE_DIR/$RTX_VERSION" "s3://rtx.pub/$RTX_VERSION/" --cache-control "$cache_week" --no-progress --recursive

aws s3 cp "$RELEASE_DIR" "s3://rtx.pub/" --cache-control "$cache_day" --no-progress --recursive --exclude "*" --include "rtx-latest-*"
aws s3 cp "$RELEASE_DIR" "s3://rtx.pub/" --cache-control "$cache_day" --no-progress --content-type "text/plain" --recursive --exclude "*" --include "SHASUMS*"
aws s3 cp "$RELEASE_DIR/VERSION" "s3://rtx.pub/" --cache-control "$cache_day" --no-progress --content-type "text/plain"
aws s3 cp "$RELEASE_DIR/install.sh" "s3://rtx.pub/" --cache-control "$cache_day" --no-progress --content-type "text/plain"
aws s3 cp "./rtx/schema/rtx.json" "s3://rtx.pub/schema/rtx.json" --cache-control "$cache_day" --no-progress --content-type "application/json"
aws s3 cp "./rtx/schema/rtx.plugin.json" "s3://rtx.pub/schema/rtx.plugin.json" --cache-control "$cache_day" --no-progress --content-type "application/json"

aws s3 cp artifacts/rpm/rtx.repo s3://rtx.pub/rpm/ --cache-control "$cache_day" --no-progress
aws s3 cp artifacts/rpm/packages/ s3://rtx.pub/rpm/packages/ --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/rpm/repodata/ s3://rtx.pub/rpm/repodata/ --cache-control "$cache_day" --no-progress --recursive --exclude "*" --include "repomd.xml*"
aws s3 cp artifacts/rpm/repodata/ s3://rtx.pub/rpm/repodata/ --cache-control "$cache_week" --no-progress --recursive --exclude "repomd.xml*"

aws s3 cp artifacts/deb/pool/ s3://rtx.pub/deb/pool/ --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/deb/dists/ s3://rtx.pub/deb/dists/ --cache-control "$cache_day" --no-progress --no-progress --recursive

export CLOUDFLARE_ACCOUNT_ID=6e243906ff257b965bcae8025c2fc344
export CLOUDFLARE_ZONE_ID=80d977fd09f01db52bec165778088891
curl -X POST "https://api.cloudflare.com/client/v4/zones/$CLOUDFLARE_ZONE_ID/purge_cache" \
	-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
	-H "Content-Type: application/json" \
	--data '{
    "prefixes": [
      "/VERSION",
      "/SHASUMS",
      "/install.sh",
      "/rtx-latest-",
      "/rpm/repodata/",
      "/deb/dists/"
    ]
  }'

#aws cloudfront create-invalidation --distribution-id E166HHA8DY7YLW --paths \
#	"/VERSION" \
#	"/SHASUMS*" \
#	"/install.sh" \
#	"/rtx-latest-*" \
#	"/rpm/repodata/*" \
#	"/deb/dists/*"
