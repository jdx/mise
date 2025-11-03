#!/usr/bin/env bash
set -euxo pipefail

CURRENT_VERSION="${1}"
echo "Current version: $CURRENT_VERSION"

# Cloudflare API endpoint for updating redirect rules
ZONE_ID="90dfd7997bdcfa8579c52d8ee8dd4cd1" # jdx.dev zone ID

# Use the correct ruleset ID and rule ID from the API response
RULESET_ID="f929f651a2824bfcac1cca11bbd3cf73"
RULE_ID="ba099b251b5647d7833d319a3f5e0416"
echo "Using ruleset ID: $RULESET_ID"
echo "Using rule ID: $RULE_ID"

# Update the redirect rule with the new version
echo "Updating redirect rule with version $CURRENT_VERSION..."

curl --fail-with-body -X PATCH "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/rulesets/$RULESET_ID/rules/$RULE_ID" \
	-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
	-H "Content-Type: application/json" \
	--data @- <<EOF
{
  "expression": "(http.host eq \"mise.jdx.dev\" and starts_with(http.request.uri.path, \"/mise-latest-\"))",
  "action": "redirect",
  "action_parameters": {
    "from_value": {
      "preserve_query_string": false,
      "status_code": 302,
      "target_url": {
        "expression": "concat(\"https://github.com/jdx/mise/releases/download/${CURRENT_VERSION}/\", wildcard_replace(http.request.uri.path, \"/mise-latest-*\", \"mise-${CURRENT_VERSION}-\${1}\"))"
      }
    }
  },
  "description": "Redirect mise-latest-* to current version",
  "enabled": false,
  "ref": "ba099b251b5647d7833d319a3f5e0416"
}
EOF

echo "Redirect rule updated successfully!"
