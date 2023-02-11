#!/usr/bin/env bash
set -euxo pipefail

cache_hour="max-age=3600,s-maxage=3600,public,immutable"
cache_day="max-age=86400,s-maxage=86400,public,immutable"
cache_week="max-age=604800,s-maxage=604800,public,immutable"

platforms=(
	linux-x64
	linux-arm64
	macos-x64
	macos-arm64
)

./rtx/scripts/render-install.sh >"$RELEASE_DIR"/install.sh
echo "$RTX_VERSION" | tr -d 'v' > "$RELEASE_DIR"/VERSION

aws s3 cp "$RELEASE_DIR/$RTX_VERSION" "s3://rtx.pub/$RTX_VERSION/" --cache-control "$cache_week" --no-progress --recursive
aws s3 cp "$RELEASE_DIR" "s3://rtx.pub/" --cache-control "$cache_hour" --no-progress --recursive --exclude "*" \
  --include "rtx-latest-*" \
  --include "SHASUMS*"     \
  --include "VERSION"      \
  --include "install.sh"

aws s3 cp artifacts/rpm/rtx.repo s3://rtx.pub/rpm/           --cache-control "$cache_day" --no-progress
aws s3 cp artifacts/rpm/packages/ s3://rtx.pub/rpm/packages/ --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/rpm/repodata/ s3://rtx.pub/rpm/repodata/ --cache-control "$cache_hour" --no-progress --recursive --exclude "*" --include "repomd.xml*"
aws s3 cp artifacts/rpm/repodata/ s3://rtx.pub/rpm/repodata/ --cache-control "$cache_week" --no-progress --recursive --exclude "repomd.xml*"

aws s3 cp artifacts/deb/pool/ s3://rtx.pub/deb/pool/   --cache-control "$cache_week" --no-progress --recursive
aws s3 cp artifacts/deb/dists/ s3://rtx.pub/deb/dists/ --cache-control "$cache_hour" --no-progress --no-progress --recursive
