#!/usr/bin/env bash
set -euo pipefail

TAG=${1:?usage: publish-version.sh <release-tag>}
if ! [[ $TAG =~ ^v[0-9][0-9A-Za-z._+-]*$ ]]; then
	echo "Invalid release tag: $TAG" >&2
	exit 1
fi

version_file=$(mktemp)
trap 'rm -f "$version_file"' EXIT
printf '%s\n' "${TAG#v}" >"$version_file"

export AWS_REGION=auto
export AWS_DEFAULT_OUTPUT=json
export AWS_ENDPOINT_URL=https://6e243906ff257b965bcae8025c2fc344.r2.cloudflarestorage.com
export AWS_ACCESS_KEY_ID=$CLOUDFLARE_ACCESS_KEY_ID
export AWS_SECRET_ACCESS_KEY=$CLOUDFLARE_SECRET_ACCESS_KEY
export AWS_RETRY_MODE=adaptive
export AWS_MAX_ATTEMPTS=10

aws s3 cp "$version_file" s3://mise/VERSION \
	--cache-control "max-age=86400,s-maxage=86400,public,immutable" \
	--no-progress \
	--content-type "text/plain"

# Purge every zone that fronts release artifacts only after VERSION points at
# the public release. This also makes the already-uploaded latest assets and
# install scripts visible in the same post-publication cache refresh.
ZONES=(
	"jdx.dev:90dfd7997bdcfa8579c52d8ee8dd4cd1"
	"en.dev:531d003297f1f4ae2415b41f7f5da8fa"
	"mise.run:782fc08181b7bbd26c529a00df52a277"
)
for entry in "${ZONES[@]}"; do
	IFS=":" read -r HOST ZONE_ID <<<"$entry"
	echo "Purging CDN cache for $HOST (zone=$ZONE_ID)"
	curl --fail-with-body -X POST "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/purge_cache" \
		-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
		-H "Content-Type: application/json" \
		--data '{ "purge_everything": true }'
done
