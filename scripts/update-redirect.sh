#!/usr/bin/env bash
set -euxo pipefail

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | cut -d'"' -f2)
echo "Current version: $CURRENT_VERSION"

# Cloudflare API endpoint for updating redirect rules
ZONE_ID="90dfd7997bdcfa8579c52d8ee8dd4cd1" # jdx.dev zone ID

# Use the known rule ID
RULE_ID="ba099b251b5647d7833d319a3f5e0416"
echo "Using redirect rule ID: $RULE_ID"

# Update the redirect rule with the new version
echo "Updating redirect rule with version $CURRENT_VERSION..."

# shellcheck disable=SC2016
# shellcheck disable=SC2086
curl --fail-with-body -X PUT "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/rulesets/$RULE_ID" \
	-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
	-H "Content-Type: application/json" \
	--data @- << EOF
{
    "name": "mise-latest redirects",
    "description": "Redirects mise-latest-* requests to current version",
    "rules": [
      {
        "expression": "(http.host eq \"mise.jdx.dev\" and starts_with(http.request.uri.path, \"/mise-latest-\"))",
        "action": "redirect",
        "action_parameters": {
          "from_value": "concat(\"https://github.com/jdx/mise/releases/download/v${CURRENT_VERSION}/\", wildcard_replace(http.request.uri.path, \"/mise-latest-*\", \"mise-v${CURRENT_VERSION}-${1}\"))"
        },
        "description": "Redirect mise-latest-* to current version",
        "enabled": false
      }
    ]
  }
EOF

echo "Redirect rule updated successfully!"
