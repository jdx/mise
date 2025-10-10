#!/usr/bin/env bash
# Generate aqua-registry changelog section
# Usage: gen-aqua-changelog.sh <old_packages> <new_packages> <heading_level>
set -euo pipefail

OLD_PKGS="$1"
NEW_PKGS="$2"
HEADING_LEVEL="${3:-###}" # Default to ### for CHANGELOG.md sections

if [[ -z $OLD_PKGS ]]; then
	exit 0
fi

# Find new packages (in new but not in old)
NEW_PACKAGES="$(comm -13 <(echo "$OLD_PKGS") <(echo "$NEW_PKGS"))"

# Find updated packages (in both old and new)
# For updated, check which files actually changed
COMMON_PKGS="$(comm -12 <(echo "$OLD_PKGS") <(echo "$NEW_PKGS"))"
UPDATED_PACKAGES=""
while IFS= read -r pkg; do
	if [[ -n $pkg ]]; then
		if git diff --quiet HEAD -- "crates/aqua-registry/aqua-registry/pkgs/$pkg/registry.yaml" 2>/dev/null; then
			: # No changes
		else
			UPDATED_PACKAGES+="$pkg"$'\n'
		fi
	fi
done <<<"$COMMON_PKGS"

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
