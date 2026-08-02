#!/usr/bin/env bash
set -euo pipefail

base_url=${1:-${MISE_TASK_CACHE_COMPAT_URL:-}}
namespace=${2:-${MISE_TASK_CACHE_COMPAT_NAMESPACE:-}}
token=${MISE_TASK_CACHE_COMPAT_TOKEN:-}

if [[ -z $base_url || -z $namespace ]]; then
	printf 'usage: %s <base-url> <disposable-namespace>\n' "$0" >&2
	printf 'or set MISE_TASK_CACHE_COMPAT_URL and MISE_TASK_CACHE_COMPAT_NAMESPACE\n' >&2
	exit 2
fi

base_url=${base_url%/}
key=$(od -An -N32 -tx1 /dev/urandom | tr -d '[:space:]')
temp_dir=$(mktemp -d)
manifest=$temp_dir/manifest.json
conflict=$temp_dir/conflict.json
artifact=$temp_dir/artifact.tar.zst
conflicting_artifact=$temp_dir/conflicting-artifact.tar.zst
response=$temp_dir/response

status_headers=(-H 'Mise-Cache-Protocol: 1')
request_headers=(
	-H 'Mise-Cache-Protocol: 1'
	-H "Mise-Cache-Namespace: $namespace"
)
isolation_headers=(
	-H 'Mise-Cache-Protocol: 1'
	-H "Mise-Cache-Namespace: ${namespace}-isolated"
)
if [[ -n $token ]]; then
	status_headers+=(-H "Authorization: Bearer $token")
	request_headers+=(-H "Authorization: Bearer $token")
	isolation_headers+=(-H "Authorization: Bearer $token")
fi

best_effort_cleanup() {
	curl -sS -o /dev/null -X DELETE "${request_headers[@]}" \
		-H 'Accept: application/vnd.mise.task-cache-manifest.v2+json' \
		"$base_url/v1/cache/$key" || true
	curl -sS -o /dev/null -X DELETE "${request_headers[@]}" \
		-H 'Accept: application/vnd.mise.task-cache-artifact.v1+zstd' \
		"$base_url/v1/cache/$key/artifact" || true
	rm -rf "${temp_dir:?}"
}
trap best_effort_cleanup EXIT

expect_code() {
	local actual=$1
	local expected=$2
	local operation=$3
	if [[ ! $actual =~ $expected ]]; then
		printf '%s returned HTTP %s; expected %s\n' "$operation" "$actual" "$expected" >&2
		exit 1
	fi
}

printf '{"format":2,"key":"%s","artifact_checksum":"blake3:compatibility-fixture","roots":["dist"],"output":[]}\n' "$key" >"$manifest"
printf '{"format":2,"key":"%s","artifact_checksum":"blake3:different","roots":["dist"],"output":[]}\n' "$key" >"$conflict"
printf 'mise remote cache compatibility artifact\n' >"$artifact"
printf 'different remote cache artifact\n' >"$conflicting_artifact"

code=$(curl -sS -o "$response" -w '%{http_code}' \
	"${status_headers[@]}" "$base_url/v1/status")
expect_code "$code" '^200$' 'status discovery'
jq -e '.protocol == 1 and .store == 1' "$response" >/dev/null

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-artifact.v1+zstd' \
	--data-binary "@$artifact" "$base_url/v1/cache/$key/artifact")
expect_code "$code" '^201$' 'artifact upload'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-manifest.v2+json' \
	--data-binary "@$manifest" "$base_url/v1/cache/$key")
expect_code "$code" '^201$' 'manifest upload'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-manifest.v2+json' \
	--data-binary "@$manifest" "$base_url/v1/cache/$key")
expect_code "$code" '^204$' 'identical manifest upload'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-artifact.v1+zstd' \
	--data-binary "@$artifact" "$base_url/v1/cache/$key/artifact")
expect_code "$code" '^204$' 'identical artifact upload'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-artifact.v1+zstd' \
	--data-binary "@$conflicting_artifact" "$base_url/v1/cache/$key/artifact")
expect_code "$code" '^409$' 'immutable artifact replacement'

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-manifest.v2+json' \
	"$base_url/v1/cache/$key")
expect_code "$code" '^200$' 'manifest download'
cmp "$manifest" "$response"

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-artifact.v1+zstd' \
	"$base_url/v1/cache/$key/artifact")
expect_code "$code" '^200$' 'artifact download'
cmp "$artifact" "$response"

code=$(curl -sS -o "$response" -w '%{http_code}' \
	"${isolation_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-manifest.v2+json' \
	"$base_url/v1/cache/$key")
expect_code "$code" '^404$' 'cross-namespace lookup'

code=$(curl -sS -o "$response" -w '%{http_code}' \
	"${isolation_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-artifact.v1+zstd' \
	"$base_url/v1/cache/$key/artifact")
expect_code "$code" '^404$' 'cross-namespace artifact lookup'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.task-cache-manifest.v2+json' \
	--data-binary "@$conflict" "$base_url/v1/cache/$key")
expect_code "$code" '^409$' 'immutable manifest replacement'

code=$(curl -sS -o "$response" -w '%{http_code}' -X DELETE "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-manifest.v2+json' \
	"$base_url/v1/cache/$key")
expect_code "$code" '^204$' 'manifest deletion'

code=$(curl -sS -o "$response" -w '%{http_code}' -X DELETE "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-artifact.v1+zstd' \
	"$base_url/v1/cache/$key/artifact")
expect_code "$code" '^(204|404)$' 'artifact deletion'

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.task-cache-manifest.v2+json' \
	"$base_url/v1/cache/$key")
expect_code "$code" '^404$' 'lookup after deletion'

trap - EXIT
rm -rf "${temp_dir:?}"
printf 'remote task cache protocol v1 compatibility checks passed\n'
