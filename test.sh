#!/usr/bin/env bash

fails=0
for i in $(grep -o 'travis.*\.svg' README.md); do
  url=$(echo $i | sed "s/\//\/repositories\//" | sed "s/svg/json/")
  curl -sq https://api.$url | grep 'last_build_result":0' > /dev/null
  if [ $? != 0 ]; then
    let "fails += $?"
    url=$(echo $i | sed 's/\.svg//')
    echo "Plugin travis build failed: $url"
  fi
done

echo "Total plugins with failed builds: $fails"

exit $fails
