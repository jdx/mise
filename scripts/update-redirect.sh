#!/usr/bin/env bash
set -euxo pipefail

CURRENT_VERSION="${1}"
echo "Current version: $CURRENT_VERSION"

# Update the mise-latest-* redirect rule in both zones (jdx.dev and en.dev).
# Each entry: "host:zone_id:ruleset_id:rule_id"
ZONES=(
	"mise.jdx.dev:90dfd7997bdcfa8579c52d8ee8dd4cd1:f929f651a2824bfcac1cca11bbd3cf73:ba099b251b5647d7833d319a3f5e0416"
	"mise.en.dev:531d003297f1f4ae2415b41f7f5da8fa:EN_DEV_RULESET_ID:EN_DEV_RULE_ID"
)

for entry in "${ZONES[@]}"; do
	IFS=":" read -r HOST ZONE_ID RULESET_ID RULE_ID <<<"$entry"
	if [[ $ZONE_ID == *_ID || $RULESET_ID == *_ID || $RULE_ID == *_ID ]]; then
		echo "Skipping $HOST: placeholder ID(s) present — fill them in before this zone can be updated."
		continue
	fi
	echo "Updating redirect rule for $HOST (zone=$ZONE_ID ruleset=$RULESET_ID rule=$RULE_ID) to version $CURRENT_VERSION..."

	curl --fail-with-body -X PATCH "https://api.cloudflare.com/client/v4/zones/$ZONE_ID/rulesets/$RULESET_ID/rules/$RULE_ID" \
		-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
		-H "Content-Type: application/json" \
		--data @- <<EOF
{
  "expression": "(http.host eq \"$HOST\" and starts_with(http.request.uri.path, \"/mise-latest-\"))",
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
  "ref": "$RULE_ID"
}
EOF

	echo "Redirect rule for $HOST updated successfully!"
done
