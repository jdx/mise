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
#   - it is not the version in s3://mise/VERSION (re-read before each removal)
# and at most PRUNE_MAX_DELETE folders (oldest first) are removed per run.
#
# Dry run unless PRUNE_RELEASES=delete. Every run first checks that the R2
# credentials can read s3://mise/VERSION and fails (exit 1) if they cannot, so a
# broken token shows up even while the stats gates below skip every run.
# Anything that prevents a trustworthy decision (stats endpoint missing, no or
# partial coverage, stale or empty stats) skips the run with exit 0 and deletes
# nothing.

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
# Decimal integers only: a leading zero would make bash arithmetic read the
# value as octal (and 08 or 09 as an error).
int_re='^-?(0|[1-9][0-9]*)$'

# int A OP B: compare two decimal integers with -lt, -gt or -ne. Anything else
# aborts the script, because a failed comparison inside `if` would otherwise
# read as false and skip a gate.
int() {
	if ! [[ $1 =~ $int_re && $3 =~ $int_re ]]; then
		echo "internal error: cannot compare '$1' $2 '$3' as integers" >&2
		exit 1
	fi
	case $2 in
	-lt) [ "$1" -lt "$3" ] ;;
	-gt) [ "$1" -gt "$3" ] ;;
	-ne) [ "$1" -ne "$3" ] ;;
	*)
		echo "internal error: unsupported comparison $2" >&2
		exit 1
		;;
	esac
}

for var in window_days min_covered_days max_stats_age_days min_age_days keep_newest max_delete; do
	if ! [[ ${!var} =~ ^(0|[1-9][0-9]*)$ ]]; then
		echo "$var must be a non-negative decimal integer without leading zeros, got '${!var}'" >&2
		exit 1
	fi
done
if int "$min_covered_days" -lt 1 || int "$keep_newest" -lt 1; then
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

# --- R2 credentials -------------------------------------------------------------
export AWS_REGION=auto
export AWS_DEFAULT_OUTPUT=json
export AWS_ENDPOINT_URL=https://6e243906ff257b965bcae8025c2fc344.r2.cloudflarestorage.com
export AWS_ACCESS_KEY_ID=${CLOUDFLARE_ACCESS_KEY_ID:?CLOUDFLARE_ACCESS_KEY_ID is required}
export AWS_SECRET_ACCESS_KEY=${CLOUDFLARE_SECRET_ACCESS_KEY:?CLOUDFLARE_SECRET_ACCESS_KEY is required}
export AWS_RETRY_MODE=adaptive
export AWS_MAX_ATTEMPTS=10

# The stats gates below skip most runs (every run until the stats cover a full
# window), so check the token on every run, before any of them. Use an object
# read, which a token scoped to Object Read & Write on this bucket can always
# do (bucket-level calls such as head-bucket may be denied). This fails the
# job in dry-run mode too.
if ! aws s3api head-object --bucket "$bucket" --key VERSION </dev/null >/dev/null 2>"$tmp/auth.err"; then
	sed 's/^/  /' "$tmp/auth.err" >&2
	echo "::error::R2 credentials check failed: could not read s3://$bucket/VERSION. Check the R2_PRUNE_ACCESS_KEY_ID and R2_PRUNE_SECRET_ACCESS_KEY secrets of the r2-prune environment (the token needs Object Read & Write on bucket $bucket)."
	printf '## Prune R2 releases: failed\n\nThe R2 credentials could not read s3://%s/VERSION. Nothing was deleted.\n' "$bucket" >>"$summary"
	exit 1
fi
echo "R2 credentials can read s3://$bucket/VERSION"

# --- download stats -----------------------------------------------------------
status=$(curl -sS --retry 3 --max-time 60 -o "$tmp/stats.json" -w '%{http_code}' "$stats_url") ||
	skip "could not reach the download stats endpoint ($stats_url)."
if [[ $status != 200 ]]; then
	skip "download stats endpoint returned HTTP $status ($stats_url)."
fi
# Before the first complete day is rolled up, the endpoint reports no coverage
# and null dates. Only that exact shape gets its own message; any other null or
# missing field still fails the format check below.
if jq -e '
	def days: type == "number" and . >= 0 and . == floor and . <= 100000;
	type == "object" and
	(.days | days) and
	.covered_days == 0 and
	has("first_covered_date") and .first_covered_date == null and
	has("last_covered_date") and .last_covered_date == null and
	((has("versions") | not) or .versions == {})
' "$tmp/stats.json" >/dev/null 2>&1; then
	skip "download stats have no covered days yet ($stats_url)."
fi
if ! jq -e '
	def days: type == "number" and . >= 0 and . == floor and . <= 100000;
	(.days | days) and
	(.covered_days | days) and
	.covered_days <= .days and
	(.last_covered_date | type == "string" and test("^[0-9]{4}-[0-9]{2}-[0-9]{2}$")) and
	(.versions | type == "object")
' "$tmp/stats.json" >/dev/null 2>&1; then
	skip "download stats response is not in the expected format ($stats_url)."
fi
# Only an absent version means zero downloads. A present entry without valid
# counts must not be read as zero, so one bad entry skips the whole run.
bad_entries=$(jq -r '
	def count: type == "number" and . >= 0 and . == floor;
	.versions | to_entries
	| map(select((.value | type == "object" and (.binary | count) and (.other | count)) | not) | .key)
	| join(", ")
' "$tmp/stats.json")
if [[ -n $bad_entries ]]; then
	skip "download stats have entries without non-negative integer binary/other counts: $bad_entries."
fi
stats_days=$(jq -r '.days' "$tmp/stats.json")
covered_days=$(jq -r '.covered_days' "$tmp/stats.json")
first_day=$(jq -r '.first_covered_date // "?"' "$tmp/stats.json")
last_day=$(jq -r '.last_covered_date // "?"' "$tmp/stats.json")
# jq can print a valid integer as 1E+2, which is not a decimal integer to bash.
if ! [[ $stats_days =~ ^(0|[1-9][0-9]*)$ && $covered_days =~ ^(0|[1-9][0-9]*)$ ]]; then
	skip "download stats have non-integer days ($stats_days) or covered_days ($covered_days)."
fi
echo "download stats cover $covered_days day(s): $first_day .. $last_day"
if int "$stats_days" -ne "$window_days"; then
	skip "download stats are for a $stats_days-day window, not the requested $window_days days ($first_day .. $last_day)."
fi
if int "$covered_days" -lt "$stats_days"; then
	skip "download stats cover only $covered_days of their $stats_days-day window ($first_day .. $last_day)."
fi
if int "$covered_days" -lt "$min_covered_days"; then
	skip "download stats cover only $covered_days of the required $min_covered_days days ($first_day .. $last_day)."
fi
# The rollup runs daily (~02:20 UTC) for the previous day. A stale window means
# the pipeline stopped, so recent downloads would be missing from the counts.
# A date that matches the shape but isn't a real day (such as 2026-99-99) makes
# strptime fail, and one like 2026-02-30 is normalized to another day, so the
# parsed date must format back to the input. Treat those, and any date in the
# future, as a bad response.
if ! stats_age_days=$(jq -n --arg d "$last_day" --argjson now "$(date -u +%s)" '
	($d | strptime("%Y-%m-%d") | mktime) as $t
	| if ($t | strftime("%Y-%m-%d")) != $d then error("not a calendar date") else . end
	| (($now - $t) / 86400) | floor
' 2>/dev/null) ||
	[[ ! $stats_age_days =~ ^(0|[1-9][0-9]*)$ ]]; then
	skip "download stats have an invalid last_covered_date: $last_day."
fi
if int "$stats_age_days" -gt "$max_stats_age_days"; then
	skip "download stats end on $last_day, $stats_age_days days ago (limit $max_stats_age_days), so they are stale."
fi
# All-zero stats mean the log pipeline is broken, not that nobody downloads mise.
if ! jq -e '([.versions[] | .binary + .other] | add // 0) > 0' "$tmp/stats.json" >/dev/null; then
	skip "download stats report zero downloads for every version, which looks like a broken log pipeline."
fi

# --- bucket listing -------------------------------------------------------------
# Prints the version in s3://mise/VERSION, or exits the script if it cannot be
# read or does not look like a release version.
read_current_version() {
	local version
	# Command substitution does not inherit errexit, so fail explicitly and never
	# fall back to a VERSION file left over from an earlier read.
	rm -f "$tmp/VERSION"
	if ! aws s3 cp "s3://$bucket/VERSION" "$tmp/VERSION" --no-progress </dev/null >/dev/null; then
		echo "could not read s3://$bucket/VERSION" >&2
		exit 1
	fi
	version=$(tr -d '[:space:]' <"$tmp/VERSION")
	version=${version#v}
	if ! [[ $version =~ ^[0-9]{4}\.[0-9]{1,2}\.[0-9]+$ ]]; then
		echo "unexpected VERSION in bucket: '$version'" >&2
		exit 1
	fi
	printf '%s\n' "$version"
}

current_version=$(read_current_version)
echo "current VERSION: $current_version"

# Every release folder starts with "v"; the strict folder regex below drops any
# other top-level key that happens to share the prefix. This relies on the AWS
# CLI's automatic pagination, which merges every page (the bucket has ~21k
# objects under v*) into one .Contents; never pass --no-paginate or --max-items.
# A truncated result would hide objects, so refuse it.
aws s3api list-objects-v2 --bucket "$bucket" --prefix v >"$tmp/listing.json"
if jq -e '.IsTruncated == true' "$tmp/listing.json" >/dev/null; then
	echo "bucket listing is truncated; refusing to continue" >&2
	exit 1
fi

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
	if int "$selected" -gt 0; then
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

deleted=0
deleted_bytes=0
while IFS=$'\t' read -r folder bytes; do
	# Re-check right before deleting so a bug above can never widen the prefix.
	if ! [[ $folder =~ ^v[0-9]{4}\.[0-9]{1,2}\.[0-9]+$ ]]; then
		echo "refusing to delete unexpected folder '$folder'" >&2
		exit 1
	fi
	# A release may have been published since the listing; never remove the
	# folder VERSION points at now. A failed read aborts the run.
	if ! latest_version=$(read_current_version); then
		exit 1
	fi
	if [[ $folder == "v$latest_version" ]]; then
		echo "skipping $folder/: it is the current VERSION"
		continue
	fi
	# The folder may have been re-uploaded since the bucket listing. List it again
	# and keep it if its newest object is now too young or the listing fails.
	# This listing is auto-paginated too.
	# (Newest-N needs no recheck: new uploads only make other folders older in
	# rank. An upload racing the rm itself is not prevented; GitHub Releases
	# still has every file.)
	if ! aws s3api list-objects-v2 --bucket "$bucket" --prefix "$folder/" </dev/null >"$tmp/folder.json" ||
		! folder_age_days=$(jq --argjson now "$(date -u +%s)" '
			def epoch: sub("\\.[0-9]+"; "") | sub("\\+00:00$"; "Z") | fromdateiso8601;
			if .IsTruncated == true then error("truncated") else . end
			| [.Contents[]?.LastModified | epoch] | if length == 0 then error("empty") else max end
			| (($now - .) / 86400) | floor
		' "$tmp/folder.json" 2>/dev/null) ||
		[[ ! $folder_age_days =~ $int_re ]]; then
		echo "skipping $folder/: could not list it again before deleting, or it is now empty"
		continue
	fi
	if int "$folder_age_days" -lt "$min_age_days"; then
		echo "skipping $folder/: newest object is now $folder_age_days days old (minimum $min_age_days)"
		continue
	fi
	echo "deleting s3://$bucket/$folder/"
	aws s3 rm "s3://$bucket/$folder/" --recursive --only-show-errors </dev/null
	deleted=$((deleted + 1))
	deleted_bytes=$((deleted_bytes + bytes))
done < <(jq -r '.[] | [.folder, .bytes] | @tsv' "$tmp/candidates.json")
echo "deleted $deleted folder(s), $(gb "$deleted_bytes")"
