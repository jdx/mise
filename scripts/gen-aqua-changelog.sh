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
	local -a tags=()

	while IFS= read -r tag; do
		if [[ $tag == "$OLD_TAG" ]]; then
			break
		fi
		if [[ $tag == "$NEW_TAG" ]]; then
			collecting=1
		fi
		if [[ $collecting -eq 1 ]]; then
			tags+=("$tag")
		fi
	done < <(gh release list --repo "$REPO" --limit 1000 --json tagName --jq '.[].tagName')

	if [[ ${#tags[@]} -eq 0 ]]; then
		echo "Unable to find aqua-registry releases from $OLD_TAG to $NEW_TAG" >&2
		return 1
	fi

	for ((i = ${#tags[@]} - 1; i >= 0; i--)); do
		printf '%s\n' "${tags[$i]}"
	done
}

link_contributors() {
	local text="$1"
	local prefix handles handle links

	if [[ ! $text =~ ^(.+[^[:space:]])[[:space:]]+(@[A-Za-z0-9_.-]+([[:space:]]+@[A-Za-z0-9_.-]+)*)[[:space:]]*$ ]]; then
		printf '%s' "$text"
		return
	fi

	prefix="${BASH_REMATCH[1]}"
	handles="${BASH_REMATCH[2]}"
	links=""

	for handle in $handles; do
		handle="${handle#@}"
		if [[ -n $links ]]; then
			links+=", "
		fi
		links+="[$handle](https://github.com/$handle)"
	done

	printf '%s (%s)' "$prefix" "$links"
}

section_title() {
	sed -E 's/^[^[:alnum:]]+[[:space:]]*//; s/[[:space:]]+$//'
}

format_release() {
	local tag="$1"
	local body section line pr text

	body="$(gh release view "$tag" --repo "$REPO" --json body --jq .body)"
	echo "#### [$tag](https://github.com/$REPO/releases/tag/$tag)"

	while IFS= read -r line; do
		line="${line%$'\r'}"
		if [[ -z $line ]] || [[ $line == "[Issues]"* ]]; then
			continue
		fi
		if [[ $line =~ ^##[[:space:]]+(.+)$ ]]; then
			section="$(printf '%s' "${BASH_REMATCH[1]}" | section_title)"
			if [[ -n $section ]]; then
				echo ""
				echo "**$section**"
				echo ""
			fi
			continue
		fi
		if [[ $line =~ ^#([0-9]+)[[:space:]]+(.+)$ ]]; then
			pr="${BASH_REMATCH[1]}"
			text="$(link_contributors "${BASH_REMATCH[2]}")"
			echo "- $text ([#$pr](https://github.com/$REPO/pull/$pr))"
			continue
		fi

		text="$(link_contributors "$line")"
		if [[ -n $text ]]; then
			echo "- $text"
		fi
	done <<<"$body"
	echo ""
}

RELEASE_TAGS="$(release_tags)"

echo "$HEADING_LEVEL 📦 Aqua Registry"
echo ""
echo "Updated [aqua-registry](https://github.com/$REPO): [$OLD_TAG](https://github.com/$REPO/releases/tag/$OLD_TAG) -> [$NEW_TAG](https://github.com/$REPO/releases/tag/$NEW_TAG)."
echo ""
echo "Changelog copied from [aqua-registry releases](https://github.com/$REPO/releases)."
echo ""

while IFS= read -r tag; do
	format_release "$tag"
done <<<"$RELEASE_TAGS"
