#!/usr/bin/env bash

fails=0
for i in $(grep -o 'travis.*\.svg' README.md); do
  url=$(echo $i | sed "s/\//\/repositories\//" | sed "s/\.svg//")
  curl -sq -H "Accept: application/json" https://api.$url | grep 'last_build_result":0' > /dev/null
  status=$?
  if [ "$status" != "0" ]; then
    fails=$((fails+${status}))
    url=$(echo $i | sed 's/\.svg//')
    echo "Plugin build failed: https://$url"
  fi
done

echo "Total plugins with failed builds: $fails"

exit $fails
