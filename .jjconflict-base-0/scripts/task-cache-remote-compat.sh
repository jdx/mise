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

for command in b3sum curl jq sha256sum; do
	if ! command -v "$command" >/dev/null; then
		printf '%s is required\n' "$command" >&2
		exit 2
	fi
done

base_url=${base_url%/}
temp_dir=$(mktemp -d)
response=$temp_dir/response
nonce=$(od -An -N16 -tx1 /dev/urandom | tr -d '[:space:]')

request_headers=(
	-H 'mise-cache-protocol: 1'
	-H "mise-cache-namespace: $namespace"
)
isolation_headers=(
	-H 'mise-cache-protocol: 1'
	-H "mise-cache-namespace: ${namespace}-isolated"
)
if [[ -n $token ]]; then
	request_headers+=(-H "Authorization: Bearer $token")
	isolation_headers+=(-H "Authorization: Bearer $token")
fi

cleanup() {
	rm -rf "${temp_dir:?}"
}
trap cleanup EXIT

expect_code() {
	local actual=$1
	local expected=$2
	local operation=$3
	if [[ ! $actual =~ $expected ]]; then
		printf '%s returned HTTP %s; expected %s\n' "$operation" "$actual" "$expected" >&2
		if [[ -s $response ]]; then
			cat "$response" >&2
		fi
		exit 1
	fi
}

digest_file() {
	local algorithm=$1
	local path=$2
	case $algorithm in
	blake3) b3sum "$path" | cut -d' ' -f1 ;;
	sha256) sha256sum "$path" | cut -d' ' -f1 ;;
	*)
		printf 'unsupported digest algorithm: %s\n' "$algorithm" >&2
		exit 2
		;;
	esac
}

blob_url() {
	local algorithm=$1
	local hash=$2
	local size=$3
	printf '%s/v1/blobs/%s/%s/%s' "$base_url" "$algorithm" "$hash" "$size"
}

put_blob() {
	local path=$1
	local label=$2
	local algorithm=$3
	local hash size code
	hash=$(digest_file "$algorithm" "$path")
	size=$(wc -c <"$path" | tr -d '[:space:]')
	code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
		-H 'If-None-Match: *' -H 'Content-Type: application/octet-stream' \
		--data-binary "@$path" "$(blob_url "$algorithm" "$hash" "$size")")
	expect_code "$code" '^(201|204)$' "$label upload"
}

printf 'mise remote cache compatibility artifact %s\n' "$nonce" >"$temp_dir/artifact"
artifact_hash=$(digest_file sha256 "$temp_dir/artifact")
artifact_size=$(wc -c <"$temp_dir/artifact" | tr -d '[:space:]')

jq -cnSj \
	--arg hash "$artifact_hash" \
	--argjson size "$artifact_size" \
	'{directories:[],files:[{digest:{algorithm:"sha256",hash:$hash,size:$size},executable:false,mode:420,name:"artifact.txt"}],symlinks:[],version:1}' \
	>"$temp_dir/directory.json"
directory_hash=$(digest_file sha256 "$temp_dir/directory.json")
directory_size=$(wc -c <"$temp_dir/directory.json" | tr -d '[:space:]')

jq -cnSj --arg nonce "$nonce" \
	'{arch:"x86_64",args:[],command_inputs:[],dependency_keys:[],environment:{},kind:"task",os:"linux",outputs:["artifact.txt"],phase:"normal",root:".",run:[{task:"compatibility"}],shell:null,source_hash:("sha256:"+$nonce),task:"remote-cache-compat",tools:[],vars:{},version:1}' \
	>"$temp_dir/action.json"
action_hash=$(digest_file blake3 "$temp_dir/action.json")
action_size=$(wc -c <"$temp_dir/action.json" | tr -d '[:space:]')

jq -cnSj --arg nonce "$nonce" \
	'{execution_duration_ns:1,kind:"task",output:[],restored_bytes:1,roots:["artifact.txt"],task_identity:("remote-cache-compat:"+$nonce),version:1}' \
	>"$temp_dir/metadata.json"
metadata_hash=$(digest_file sha256 "$temp_dir/metadata.json")
metadata_size=$(wc -c <"$temp_dir/metadata.json" | tr -d '[:space:]')

jq -cnSj \
	--arg action_hash "$action_hash" --argjson action_size "$action_size" \
	--arg directory_hash "$directory_hash" --argjson directory_size "$directory_size" \
	--arg metadata_hash "$metadata_hash" --argjson metadata_size "$metadata_size" \
	'{action:{algorithm:"blake3",hash:$action_hash,size:$action_size},metadata:{algorithm:"sha256",hash:$metadata_hash,size:$metadata_size},output_root:{algorithm:"sha256",hash:$directory_hash,size:$directory_size},version:1}' \
	>"$temp_dir/result.json"

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" "$base_url/v1/status")
expect_code "$code" '^200$' 'status discovery'

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" "$base_url/v1/capabilities")
expect_code "$code" '^200$' 'capability discovery'
jq -e '
	.protocol.major == 1 and
	(.digest_algorithms | index("sha256")) != null and
	.action_kinds.task.action_schema == 1 and
	.action_kinds.task.metadata_schema == 1
' "$response" >/dev/null

missing_request=$(jq -cn \
	--arg hash "$action_hash" --argjson size "$action_size" \
	'{digests:[{algorithm:"blake3",hash:$hash,size:$size}]}')
code=$(curl -sS -o "$response" -w '%{http_code}' -X POST "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.cache-digests.v1+json' \
	--data-binary "$missing_request" "$base_url/v1/blobs:missing")
expect_code "$code" '^200$' 'missing blob query'
jq -e '.missing | length == 1' "$response" >/dev/null

put_blob "$temp_dir/artifact" 'artifact blob' sha256
put_blob "$temp_dir/directory.json" 'directory object' sha256
put_blob "$temp_dir/action.json" 'action descriptor' blake3
put_blob "$temp_dir/metadata.json" 'client metadata' sha256

code=$(curl -sS -o "$response" -w '%{http_code}' -X POST "${request_headers[@]}" \
	-H 'Content-Type: application/vnd.mise.cache-digests.v1+json' \
	--data-binary "$missing_request" "$base_url/v1/blobs:missing")
expect_code "$code" '^200$' 'present blob query'
jq -e '.missing | length == 0' "$response" >/dev/null

result_url="$base_url/v1/action-results/blake3/$action_hash/$action_size"
code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'If-None-Match: *' -H 'Content-Type: application/vnd.mise.cache-action-result.v1+json' \
	--data-binary "@$temp_dir/result.json" "$result_url")
expect_code "$code" '^201$' 'action result commit'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'If-None-Match: *' -H 'Content-Type: application/vnd.mise.cache-action-result.v1+json' \
	--data-binary "@$temp_dir/result.json" "$result_url")
expect_code "$code" '^204$' 'identical action result commit'

code=$(curl -sS -o "$response" -w '%{http_code}' "${request_headers[@]}" \
	-H 'Accept: application/vnd.mise.cache-action-result.v1+json' "$result_url")
expect_code "$code" '^200$' 'action result download'
jq -e --arg hash "$action_hash" '.action.hash == $hash and .version == 1' "$response" >/dev/null

code=$(curl -sS -D "$temp_dir/blob-headers" -o "$response" -w '%{http_code}' "${request_headers[@]}" \
	"$(blob_url sha256 "$artifact_hash" "$artifact_size")")
if [[ $code == 307 ]]; then
	redirect_url=$(awk 'BEGIN { IGNORECASE = 1 } /^Location:/ { sub(/\r$/, ""); sub(/^[^:]+:[[:space:]]*/, ""); print; exit }' "$temp_dir/blob-headers")
	if [[ $redirect_url != https://* ]]; then
		printf 'artifact redirect must use an absolute HTTPS URL\n' >&2
		exit 1
	fi
	code=$(curl -sS -o "$response" -w '%{http_code}' "$redirect_url")
fi
expect_code "$code" '^200$' 'artifact blob download'
cmp "$temp_dir/artifact" "$response"

code=$(curl -sS -o "$response" -w '%{http_code}' "${isolation_headers[@]}" "$result_url")
expect_code "$code" '^(403|404)$' 'namespace isolation'

code=$(curl -sS -o "$response" -w '%{http_code}' -X PUT "${request_headers[@]}" \
	-H 'Content-Type: application/octet-stream' --data-binary "@$temp_dir/action.json" \
	"$(blob_url blake3 "$action_hash" "$action_size")")
expect_code "$code" '^412$' 'missing immutable precondition'

printf 'remote cache protocol v1 conformance checks passed\n'
