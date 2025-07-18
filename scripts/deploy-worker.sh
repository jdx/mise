#!/usr/bin/env bash
set -euxo pipefail

if [ -z "${CLOUDFLARE_API_TOKEN:-}" ]; then
	echo "Error: CLOUDFLARE_API_TOKEN environment variable is required"
	exit 1
fi

ACCOUNT_ID="6e243906ff257b965bcae8025c2fc344"
WORKER_NAME="mise-run"

echo "Deploying updated worker code for mise.run to worker: $WORKER_NAME"

if [[ $DRY_RUN != 1 ]]; then
	# Upload the worker script
	response=$(curl -s -X PUT "https://api.cloudflare.com/client/v4/accounts/$ACCOUNT_ID/workers/scripts/$WORKER_NAME/content" \
		-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
		-H "Content-Type: application/javascript" \
		--data-binary @cloudflare/workers/mise-run.js)

	if echo "$response" | jq -e '.success == true' >/dev/null; then
		echo "✅ Worker deployed successfully!"
	else
		echo "❌ Worker deployment failed:"
		echo "$response" | jq .
		exit 1
	fi
fi

# Show current routes
echo ""
echo "Current worker routes:"
curl -s -X GET "https://api.cloudflare.com/client/v4/accounts/$ACCOUNT_ID/workers/scripts/$WORKER_NAME/routes" \
	-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
	-H "Content-Type: application/json" | jq -r '.result[]?.pattern // "No routes found"'
