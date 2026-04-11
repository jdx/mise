#!/usr/bin/env bash
# Generate aqua-registry changelog section
# Usage: gen-aqua-changelog.sh <old_tag> <new_tag> <heading_level>
set -euo pipefail

OLD_TAG="$1"
NEW_TAG="$2"
HEADING_LEVEL="${3:-###}" # Default to ### for CHANGELOG.md sections
REPO="aquaproj/aqua-registry"

if [[ -z $OLD_TAG ]] || [[ -z $NEW_TAG ]] || [[ $OLD_TAG == "$NEW_TAG" ]]; then
	exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
	echo "gh is required to generate aqua-registry changelog entries" >&2
	exit 1
fi

release_tags() {
	local collecting=0
	local found_old=0
	local found_new=0
	local -a tags=()

	while IFS= read -r tag; do
		if [[ $tag == "$OLD_TAG" ]]; then
			found_old=1
			break
		fi
		if [[ $tag == "$NEW_TAG" ]]; then
			collecting=1
			found_new=1
		fi
		if [[ $collecting -eq 1 ]]; then
			tags+=("$tag")
		fi
	done < <(gh release list --repo "$REPO" --limit 1000 --json tagName --jq '.[].tagName')

	if [[ $found_old -eq 0 ]]; then
		return 0
	fi

	if [[ $found_new -eq 0 ]]; then
		echo "Unable to find aqua-registry release $NEW_TAG" >&2
		return 1
	fi

	if [[ ${#tags[@]} -eq 0 ]]; then
		echo "Unable to find aqua-registry releases from $OLD_TAG to $NEW_TAG" >&2
		return 1
	fi

	for ((i = ${#tags[@]} - 1; i >= 0; i--)); do
		printf '%s\n' "${tags[$i]}"
	done
}

RELEASE_TAGS="$(release_tags)"
if [[ -z $RELEASE_TAGS ]]; then
	exit 0
fi

echo "$HEADING_LEVEL 📦 Aqua Registry"
echo ""
echo "Updated [aqua-registry](https://github.com/$REPO): [$OLD_TAG](https://github.com/$REPO/releases/tag/$OLD_TAG) -> [$NEW_TAG](https://github.com/$REPO/releases/tag/$NEW_TAG)."
echo ""
echo "Included aqua-registry releases:"
echo ""

while IFS= read -r tag; do
	echo "- [$tag](https://github.com/$REPO/releases/tag/$tag)"
done <<<"$RELEASE_TAGS"
