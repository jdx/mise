#!/usr/bin/env bash
set -euxo pipefail

export CLOUDFLARE_ACCOUNT_ID=6e243906ff257b965bcae8025c2fc344
export CLOUDFLARE_ZONE_ID=80d977fd09f01db52bec165778088891
cache_hour="max-age=3600,s-maxage=3600,public,immutable"
cache_day="max-age=86400,s-maxage=86400,public,immutable"
cache_week="max-age=604800,s-maxage=604800,public,immutable"

./rtx/scripts/render-install.sh >"$RELEASE_DIR/install.sh"
echo "$RTX_VERSION" | tr -d 'v' >"$RELEASE_DIR/VERSION"
cd "$RELEASE_DIR"

cp "rtx-latest-linux-x64" "rtx-latest-linux-amd64"
cp "rtx-latest-macos-x64" "rtx-latest-macos-amd64"

fd "$RTX_VERSION" -tfile -x wrangler r2 object put -f {} rtx/{} --cc "$cache_hour"

for f in rtx-latest-*; do
	wrangler r2 object put -f "$f" "rtx/$f" --cc "$cache_hour"
done

for f in SHASUMS* VERSION install.sh; do
	wrangler r2 object put -f "$f" "rtx/$f" --cc "$cache_hour" --ct text/plain
done

cd ../artifacts
wrangler r2 object put -f rpm/rtx.repo rtx/rpm/rtx.repo --cc "$cache_day"
fd . rpm/repodata rpm/packages -tfile -E repomd.xml\* -x wrangler r2 object put -f {} rtx/{} --cc "$cache_week"
fd -g "repomd.xml*" -tfile -x wrangler r2 object put -f {} rtx/{} --cc "$cache_hour" --ct text/xml

fd . deb/dists -tfile -x wrangler r2 object put -f {} rtx/{} --cc "$cache_hour"
fd . deb/pool -tfile -x wrangler r2 object put -f {} rtx/{} --cc "$cache_week"

curl -X POST https://api.cloudflare.com/client/v4/zones/$CLOUDFLARE_ZONE_ID/purge_cache \
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
