#!/usr/bin/env bash
# Generate aqua-registry changelog section
# Usage: gen-aqua-changelog.sh <old_registry_yaml> <new_registry_yaml> <heading_level>
set -euo pipefail

OLD_REGISTRY="$1"
NEW_REGISTRY="$2"
HEADING_LEVEL="${3:-###}" # Default to ### for CHANGELOG.md sections

if [[ ! -s $OLD_REGISTRY ]]; then
	exit 0
fi

registry_manifest() {
	local registry_file="$1"
	local yq_bin="${YQ:-yq}"

	if ! command -v "$yq_bin" >/dev/null 2>&1; then
		echo "yq is required to generate aqua-registry changelog entries" >&2
		return 1
	fi

	"$yq_bin" -r '.packages[] | [(.name // (.repo_owner + "/" + .repo_name)), (. | @json)] | @tsv' "$registry_file" |
		while IFS=$'\t' read -r id package_json; do
			if [[ -n $id ]]; then
				hash="$(printf '%s' "$package_json" | sha256sum | awk '{print $1}')"
				printf '%s\t%s\n' "$id" "$hash"
			fi
		done |
		sort
}

OLD_MANIFEST="$(mktemp)"
NEW_MANIFEST="$(mktemp)"
trap 'rm -f "$OLD_MANIFEST" "$NEW_MANIFEST"' EXIT

registry_manifest "$OLD_REGISTRY" >"$OLD_MANIFEST"
registry_manifest "$NEW_REGISTRY" >"$NEW_MANIFEST"

# Find new packages (in new but not in old)
NEW_PACKAGES="$(comm -13 <(cut -f1 "$OLD_MANIFEST") <(cut -f1 "$NEW_MANIFEST"))"

# Find updated packages (same package ID but different serialized YAML)
UPDATED_PACKAGES="$(
	join -t $'\t' -j 1 "$OLD_MANIFEST" "$NEW_MANIFEST" |
		awk -F '\t' '$2 != $3 {print $1}'
)"

NEW_COUNT=$(echo "$NEW_PACKAGES" | grep -c . || true)
if [[ -z $NEW_COUNT ]] || [[ $NEW_COUNT -eq 0 ]]; then
	NEW_COUNT=0
fi

UPDATED_COUNT=$(echo "$UPDATED_PACKAGES" | grep -c . || true)
if [[ -z $UPDATED_COUNT ]] || [[ $UPDATED_COUNT -eq 0 ]]; then
	UPDATED_COUNT=0
fi

# If no changes, exit
if [[ $NEW_COUNT -eq 0 ]] && [[ $UPDATED_COUNT -eq 0 ]]; then
	exit 0
fi

# Build markdown output
echo "$HEADING_LEVEL 📦 Aqua Registry Updates"
echo ""

if [[ $NEW_COUNT -gt 0 ]]; then
	echo "#### New Packages ($NEW_COUNT)"
	echo ""
	while IFS= read -r pkg; do
		if [[ -n $pkg ]]; then
			echo "- [\`$pkg\`](https://github.com/$pkg)"
		fi
	done <<<"$NEW_PACKAGES"
	echo ""
fi

if [[ $UPDATED_COUNT -gt 0 ]]; then
	echo "#### Updated Packages ($UPDATED_COUNT)"
	echo ""
	while IFS= read -r pkg; do
		if [[ -n $pkg ]]; then
			echo "- [\`$pkg\`](https://github.com/$pkg)"
		fi
	done <<<"$UPDATED_PACKAGES"
	echo ""
fi
