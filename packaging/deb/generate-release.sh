#!/bin/bash
set -euo pipefail
# shellcheck disable=SC2044
# shellcheck disable=SC2066
# shellcheck disable=SC2086
# shellcheck disable=SC2185

do_hash() {
	HASH_NAME=$1
	HASH_CMD=$2
	echo "${HASH_NAME}:"
	for f in $(find -type f); do
		f=$(echo $f | cut -c3-) # remove ./ prefix
		if [ "$f" = "Release" ]; then
			continue
		fi
		echo " $(${HASH_CMD} ${f} | cut -d" " -f1) $(wc -c $f)"
	done
}

cat <<EOF
Origin: mise repository
Label: mise
Suite: stable
Codename: stable
Version: 1.0
Architectures: amd64 arm64
Components: main
Description: https://github.com/jdx/mise
Date: $(date -Ru)
EOF
do_hash "MD5Sum" "md5sum"
do_hash "SHA1" "sha1sum"
do_hash "SHA256" "sha256sum"
