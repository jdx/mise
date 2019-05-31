#!/usr/bin/env bash

fails=0
skips=0
total=0
while read -r -a plugin; do
  total=$((total+1))
  travis_web_url="https://${plugin[0]}/${plugin[1]}"
  if [[ ${plugin[0]} =~ .com ]]; then
    skips=$((skips+1))
    echo "Plugin build check skipped: ${travis_web_url} -- travis-ci.com (enterprise) checks are unsupported"
    continue
  fi
  url="https://api.${plugin[0]}/repos/${plugin[1]}/branches/${plugin[2]}"
  curl -sq ${url} | grep '"state":"passed"' &> /dev/null
  status=$?
  if [[ ${status} != 0 ]]; then
    fails=$((fails+1))
    echo "Plugin build check failed:  ${travis_web_url}"
  fi
done < <(grep -o 'travis.*\.svg[^)]*' README.md | sed "s~https?://~~; s:/: :; s/\.svg//; s/\?branch=/ /")

echo
echo "Plugins available: ${total}"
echo "Plugin build checks passed: $((total-fails))"
echo "Plugin build checks skipped: ${skips}"
echo "Plugin build checks failed: ${fails}"

exit ${fails}