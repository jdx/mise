#!/usr/bin/env bash
set -euo pipefail

notes_file="${1:?usage: scripts/github-release-assets.sh NOTES_FILE}"

version="$(./scripts/get-version.sh)"
release_dir="releases/$version"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY must be set}"
title="${RELEASE_TITLE:?RELEASE_TITLE must be set}"

if [[ ! -d "$release_dir" ]]; then
	echo "::error::release directory $release_dir does not exist"
	exit 1
fi

expected_assets="$(mktemp)"
actual_assets="$(mktemp)"
trap 'rm -f "$expected_assets" "$actual_assets"' EXIT

find "$release_dir" -maxdepth 1 -type f -printf "%f\n" | sort >"$expected_assets"

if [[ ! -s "$expected_assets" ]]; then
	echo "::error::release directory $release_dir has no assets"
	exit 1
fi

release_json="$(mktemp)"
trap 'rm -f "$expected_assets" "$actual_assets" "$release_json"' EXIT

if gh api "repos/$repo/releases/tags/$version" >"$release_json" 2>/dev/null; then
	if [[ "$(jq -r ".draft" <"$release_json")" != "true" ]]; then
		echo "::error::Release $version is already published; immutable release assets cannot be repaired in place"
		exit 1
	fi
	echo "Updating existing release $version"
	gh release edit "$version" --title "$title" --notes-file "$notes_file"
else
	echo "Creating draft release $version"
	gh release create "$version" \
		--title "$title" \
		--notes-file "$notes_file" \
		--verify-tag \
		--draft
fi

release_id="$(gh api "repos/$repo/releases/tags/$version" --jq ".id")"

echo "::group::Delete stale GitHub release assets"
while IFS=$'\t' read -r asset_id asset_name asset_state; do
	if ! grep -Fxq "$asset_name" "$expected_assets"; then
		echo "Deleting unexpected asset $asset_name"
		gh api -X DELETE "repos/$repo/releases/assets/$asset_id"
	elif [[ "$asset_state" != "uploaded" ]]; then
		echo "Deleting incomplete asset $asset_name (state=$asset_state)"
		gh api -X DELETE "repos/$repo/releases/assets/$asset_id"
	fi
done < <(gh api "repos/$repo/releases/$release_id/assets?per_page=100" --paginate --jq '.[] | [.id, .name, .state] | @tsv')
echo "::endgroup::"

echo "::group::Upload GitHub release assets"
while IFS= read -r asset; do
	echo "Uploading $(basename "$asset")"
	gh release upload "$version" "$asset" --clobber
done < <(find "$release_dir" -maxdepth 1 -type f | sort)
echo "::endgroup::"

echo "::group::Verify GitHub release assets"
gh api "repos/$repo/releases/$release_id/assets?per_page=100" --paginate \
	--jq '.[] | select(.state == "uploaded") | .name' | sort >"$actual_assets"

missing_assets="$(comm -23 "$expected_assets" "$actual_assets" || true)"
if [[ -n "$missing_assets" ]]; then
	echo "::error::missing GitHub release assets:"
	echo "$missing_assets"
	exit 1
fi

incomplete_assets="$(gh api "repos/$repo/releases/$release_id/assets?per_page=100" --paginate \
	--jq '.[] | select(.state != "uploaded") | "\(.name) state=\(.state)"')"
if [[ -n "$incomplete_assets" ]]; then
	echo "::error::incomplete GitHub release assets:"
	echo "$incomplete_assets"
	exit 1
fi

unexpected_assets="$(comm -13 "$expected_assets" "$actual_assets" || true)"
if [[ -n "$unexpected_assets" ]]; then
	echo "::error::unexpected GitHub release assets:"
	echo "$unexpected_assets"
	exit 1
fi

echo "Verified $(wc -l <"$expected_assets") GitHub release assets for $version"
echo "::endgroup::"
