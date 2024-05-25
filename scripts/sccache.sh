#!/bin/bash

set -x
set -euo pipefail

# shellcheck disable=SC1091
. lib.sh

main() {
  local triple
  local tag
  local td
  local url="https://github.com/mozilla/sccache"
  triple="${1}"
  triple="${triple%%-musl}"
  triple="${triple%%-musleabi}"
  triple="${triple%%-musleabihf}"
  triple="${triple%%-gnu}"
  triple="${triple%%-gnueabi}"
  triple="${triple%%-gnueabihf}"
  triple="$triple-musl"

  install_packages unzip tar

  # Download our package, then install our binary.
  td="$(mktemp -d)"
  pushd "${td}"
  tag=$(git ls-remote --tags --refs --exit-code \
    "${url}" \
    | cut -d/ -f3 \
    | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' \
    | sort --version-sort \
    | tail -n1)
  curl -LSfs "${url}/releases/download/${tag}/sccache-${tag}-${triple}.tar.gz" \
    -o sccache.tar.gz
  tar -xvf sccache.tar.gz
  rm sccache.tar.gz
  cp "sccache-${tag}-${triple}/sccache" "/usr/bin/sccache"
  chmod +x "/usr/bin/sccache"

  # clean up our install
  purge_packages
  popd
  rm -rf "${td}"
  rm "${0}"
}

main "${@}"
