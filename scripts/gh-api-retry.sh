#!/usr/bin/env bash
# Shared release-publication GitHub retry policy. Source this file.

# Publishing a release fans out across many jobs that all spend the
# repository's hourly GITHUB_TOKEN budget, so this is the call in the
# release most likely to fail for reasons unrelated to the release
# itself. Retry rather than leaving VERSION behind over one 403.
#
# An exhausted primary rate limit is only cleared by its reset, so a
# short backoff cannot help and the wait is taken instead. Checking
# the API directly rather than bootstrapping wait-for-gh-rate-limit,
# since installing that helper would itself need the API access this
# is waiting for. /rate_limit does not consume quota, so it stays
# readable while the token is exhausted. Cap the sleep so a bogus
# reset cannot park a runner indefinitely.
MAX_RATE_LIMIT_WAIT=3900

wait_before_retry() {
	local attempt="$1" status remaining="" reset="" now wait_for
	if status=$(gh api rate_limit --jq '"\(.resources.core.remaining) \(.resources.core.reset)"' 2>/dev/null); then
		remaining="${status%% *}"
		reset="${status##* }"
	fi
	now=$(date +%s)
	if [[ "$remaining" =~ ^[0-9]+$ && "$reset" =~ ^[0-9]+$ ]] &&
		((remaining == 0 && reset > now)); then
		wait_for=$((reset - now + 5))
		if ((wait_for > MAX_RATE_LIMIT_WAIT)); then
			wait_for=$MAX_RATE_LIMIT_WAIT
		fi
		echo "::warning::GitHub API quota exhausted; waiting ${wait_for}s for the window to reset" >&2
	else
		wait_for=$((5 * 2 ** (attempt - 1)))
		echo "::warning::gh api failed (attempt $attempt/5); retrying in ${wait_for}s" >&2
	fi
	sleep "$wait_for"
}

gh_api() {
	local attempt out
	for attempt in 1 2 3 4 5; do
		if out=$(gh api "$@"); then
			printf '%s' "$out"
			return 0
		fi
		if ((attempt == 5)); then
			break
		fi
		wait_before_retry "$attempt"
	done
	echo "::error::gh api $1 failed after 5 attempts" >&2
	return 1
}
