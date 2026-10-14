#!/usr/bin/env bash
set -euo pipefail

# Delete old release folders (s3://mise/v<YYYY>.<M>.<P>/) from the R2 bucket that
# serves mise.jdx.dev and mise.en.dev once nobody downloads them anymore. GitHub
# Releases keeps every asset; this only removes the CDN copy.
#
# A folder is deleted only when ALL of these hold:
#   - the download stats cover every day of their window (covered_days >= days),
#     at least PRUNE_MIN_COVERED_DAYS complete days, and end no more than
#     PRUNE_MAX_STATS_AGE_DAYS days ago
#   - its newest object is older than PRUNE_MIN_AGE_DAYS
#   - it had zero downloads (binary + other) during the stats window
#   - it is not among the PRUNE_KEEP_NEWEST most recently uploaded folders
#   - it is not the version in s3://mise/VERSION
# and at most PRUNE_MAX_DELETE folders (oldest first) are removed per run.
#
# Dry run unless PRUNE_RELEASES=delete. Anything that prevents a trustworthy
# decision (stats endpoint missing, partial coverage, stale or empty stats) skips the run
# with exit 0 and deletes nothing.

mode=${PRUNE_RELEASES:-dry-run}
window_days=${PRUNE_WINDOW_DAYS:-30}
min_covered_days=${PRUNE_MIN_COVERED_DAYS:-$window_days}
max_stats_age_days=${PRUNE_MAX_STATS_AGE_DAYS:-3}
min_age_days=${PRUNE_MIN_AGE_DAYS:-180}
keep_newest=${PRUNE_KEEP_NEWEST:-30}
max_delete=${PRUNE_MAX_DELETE:-40}
stats_url=${PRUNE_STATS_URL:-https://mise-versions.jdx.dev/api/mise-releases/downloads?days=$window_days}
bucket=mise
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

case $mode in
dry-run | delete) ;;
*)
	echo "PRUNE_RELEASES must be 'dry-run' or 'delete', got '$mode'" >&2
	exit 1
	;;
esac
for var in window_days min_covered_days max_stats_age_days min_age_days keep_newest max_delete; do
	if ! [[ ${!var} =~ ^[0-9]+$ ]]; then
		echo "$var must be a non-negative integer, got '${!var}'" >&2
		exit 1
	fi
done
if ((min_covered_days < 1 || keep_newest < 1)); then
	echo "PRUNE_MIN_COVERED_DAYS and PRUNE_KEEP_NEWEST must be at least 1" >&2
	exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

skip() {
	echo "Skipping, nothing deleted: $1"
	printf '## Prune R2 releases: skipped\n\n%s\n\nNothing was deleted.\n' "$1" >>"$summary"
	exit 0
}

echo "mode=$mode window=${window_days}d min_covered=${min_covered_days}d max_stats_age=${max_stats_age_days}d min_age=${min_age_days}d keep_newest=$keep_newest max_delete=$max_delete"

# --- download stats -----------------------------------------------------------
status=$(curl -sS --retry 3 --max-time 60 -o "$tmp/stats.json" -w '%{http_code}' "$stats_url") ||
	skip "could not reach the download stats endpoint ($stats_url)."
if [[ $status != 200 ]]; then
	skip "download stats endpoint returned HTTP $status ($stats_url)."
fi
if ! jq -e '
	(.days | type == "number") and
	(.covered_days | type == "number") and
	(.last_covered_date | type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}$")) and
	(.versions | type == "object") and
	all(.versions[]; (.binary // 0 | type == "number") and (.other // 0 | type == "number"))
' "$tmp/stats.json" >/dev/null 2>&1; then
	skip "download stats response is not in the expected format ($stats_url)."
fi
stats_days=$(jq -r '.days' "$tmp/stats.json")
covered_days=$(jq -r '.covered_days' "$tmp/stats.json")
first_day=$(jq -r '.first_covered_date // "?"' "$tmp/stats.json")
last_day=$(jq -r '.last_covered_date // "?"' "$tmp/stats.json")
echo "download stats cover $covered_days day(s): $first_day .. $last_day"
if ((covered_days < stats_days)); then
	skip "download stats cover only $covered_days of their $stats_days-day window ($first_day .. $last_day)."
fi
if ((covered_days < min_covered_days)); then
	skip "download stats cover only $covered_days of the required $min_covered_days days ($first_day .. $last_day)."
fi
# The rollup runs daily (~02:20 UTC) for the previous day. A stale window means
# the pipeline stopped, so recent downloads would be missing from the counts.
stats_age_days=$(jq -n --arg d "$last_day" --argjson now "$(date -u +%s)" \
	'(($now - ($d | strptime("%Y-%m-%d") | mktime)) / 86400) | floor')
if ((stats_age_days > max_stats_age_days)); then
	skip "download stats end on $last_day, $stats_age_days days ago (limit $max_stats_age_days), so they are stale."
fi
# All-zero stats mean the log pipeline is broken, not that nobody downloads mise.
if ! jq -e '([.versions[] | (.binary // 0) + (.other // 0)] | add // 0) > 0' "$tmp/stats.json" >/dev/null; then
	skip "download stats report zero downloads for every version, which looks like a broken log pipeline."
fi

# --- bucket listing -------------------------------------------------------------
export AWS_REGION=auto
export AWS_DEFAULT_OUTPUT=json
export AWS_ENDPOINT_URL=https://6e243906ff257b965bcae8025c2fc344.r2.cloudflarestorage.com
export AWS_ACCESS_KEY_ID=${CLOUDFLARE_ACCESS_KEY_ID:?CLOUDFLARE_ACCESS_KEY_ID is required}
export AWS_SECRET_ACCESS_KEY=${CLOUDFLARE_SECRET_ACCESS_KEY:?CLOUDFLARE_SECRET_ACCESS_KEY is required}
export AWS_RETRY_MODE=adaptive
export AWS_MAX_ATTEMPTS=10

aws s3 cp "s3://$bucket/VERSION" "$tmp/VERSION" --no-progress >/dev/null
current_version=$(tr -d '[:space:]' <"$tmp/VERSION")
current_version=${current_version#v}
if ! [[ $current_version =~ ^[0-9]{4}\.[0-9]{1,2}\.[0-9]+$ ]]; then
	echo "unexpected VERSION in bucket: '$current_version'" >&2
	exit 1
fi
echo "current VERSION: $current_version"

# Every release folder starts with "v"; the strict folder regex below drops any
# other top-level key that happens to share the prefix.
aws s3api list-objects-v2 --bucket "$bucket" --prefix v >"$tmp/listing.json"

# Age comes from the newest object in each folder. Uploads happen at release
# time and re-uploads only make a folder look younger, so this never makes a
# folder look older than its release. "Newest N" uses the same order rather
# than comparing version strings.
jq \
	--slurpfile stats "$tmp/stats.json" \
	--arg current "$current_version" \
	--argjson now "$(date -u +%s)" \
	--argjson min_age "$min_age_days" \
	--argjson keep "$keep_newest" '
	def folder_re: "^(?<folder>v(?<version>[0-9]{4}\\.[0-9]{1,2}\\.[0-9]+))/";
	def epoch: sub("\\.[0-9]+"; "") | sub("\\+00:00$"; "Z") | fromdateiso8601;
	$stats[0].versions as $downloads
	| (.Contents // [])
	| map(select(.Key | test(folder_re)) + (.Key | capture(folder_re)))
	| group_by(.folder)
	| map({
		folder: .[0].folder,
		version: .[0].version,
		files: length,
		bytes: (map(.Size) | add),
		newest: (map(.LastModified | epoch) | max)
	})
	| sort_by(.newest, .folder) | reverse
	| to_entries
	| map(.value + {rank: (.key + 1)})
	| map(. as $f
		| ($downloads[$f.version] // $downloads["v" + $f.version] // {}) as $d
		| $f + {
			age_days: ((($now - $f.newest) / 86400) | floor),
			uploaded: ($f.newest | todate | .[0:10]),
			binary: ($d.binary // 0),
			other: ($d.other // 0)
		})
	| map(. + {keep: (
		if .version == $current then "current VERSION"
		elif .rank <= $keep then "newest \($keep)"
		elif .age_days < $min_age then "younger than \($min_age)d"
		elif .binary + .other > 0 then "downloaded"
		else null end)})
' "$tmp/listing.json" >"$tmp/folders.json"

jq --argjson max "$max_delete" \
	'map(select(.keep == null)) | sort_by(.newest, .folder) | .[0:$max]' \
	"$tmp/folders.json" >"$tmp/candidates.json"

total_folders=$(jq length "$tmp/folders.json")
eligible=$(jq 'map(select(.keep == null)) | length' "$tmp/folders.json")
selected=$(jq length "$tmp/candidates.json")
selected_bytes=$(jq 'map(.bytes) | add // 0' "$tmp/candidates.json")
total_bytes=$(jq 'map(.bytes) | add // 0' "$tmp/folders.json")
gb() { awk -v b="$1" 'BEGIN { printf "%.2f GB", b / 1e9 }'; }

echo
echo "release folders: $total_folders ($(gb "$total_bytes"))"
jq -r 'group_by(.keep // "eligible for deletion") | .[] | "  \(.[0].keep // "eligible for deletion"): \(length)"' "$tmp/folders.json"
echo "selected this run: $selected of $eligible eligible, $(gb "$selected_bytes") (cap $max_delete)"
echo
jq -r '.[] | "\(.folder)/\tage=\(.age_days)d\tuploaded=\(.uploaded)\tfiles=\(.files)\tsize=\(.bytes / 1e6 | floor)MB\tbinary=\(.binary)\tother=\(.other)"' \
	"$tmp/candidates.json"

{
	if [[ $mode == delete ]]; then
		echo "## Prune R2 releases: deleting $selected folder(s)"
	else
		echo "## Prune R2 releases: dry run"
	fi
	echo
	echo "- Download stats: $covered_days day(s), $first_day .. $last_day"
	echo "- Release folders in bucket: $total_folders ($(gb "$total_bytes"))"
	echo "- Eligible for deletion: $eligible; selected this run: $selected (cap $max_delete), $(gb "$selected_bytes")"
	echo "- Rules: older than ${min_age_days}d, zero downloads, not among newest $keep_newest, not VERSION ($current_version)"
	echo
	echo "| Kept because | Folders |"
	echo "| --- | ---: |"
	jq -r 'map(select(.keep != null)) | group_by(.keep) | .[] | "| \(.[0].keep) | \(length) |"' "$tmp/folders.json"
	if ((selected > 0)); then
		echo
		echo "| Folder | Uploaded | Age (days) | Files | Size (MB) | Binary | Other |"
		echo "| --- | --- | ---: | ---: | ---: | ---: | ---: |"
		jq -r '.[] | "| `\(.folder)/` | \(.uploaded) | \(.age_days) | \(.files) | \(.bytes / 1e6 | floor) | \(.binary) | \(.other) |"' "$tmp/candidates.json"
	fi
} >>"$summary"

if [[ $mode != delete ]]; then
	echo
	echo "Dry run: nothing deleted. Set PRUNE_RELEASES=delete to delete the folders above."
	exit 0
fi

jq -r '.[].folder' "$tmp/candidates.json" | while read -r folder; do
	# Re-check right before deleting so a bug above can never widen the prefix.
	if ! [[ $folder =~ ^v[0-9]{4}\.[0-9]{1,2}\.[0-9]+$ ]] || [[ $folder == "v$current_version" ]]; then
		echo "refusing to delete unexpected folder '$folder'" >&2
		exit 1
	fi
	echo "deleting s3://$bucket/$folder/"
	aws s3 rm "s3://$bucket/$folder/" --recursive --only-show-errors
done
echo "deleted $selected folder(s), $(gb "$selected_bytes")"
