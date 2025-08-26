#!/usr/bin/env fish
# shellcheck disable=SC1072,SC1065,SC1064,SC1073,SC2103

set -l fish_trace 1
mise install tiny@3.1.0 tiny@2.0.0
or exit

echo >.mise.toml '
[tools]
tiny = "3.1.0"
[env]
FOO = "bar"
'

mkdir subdir
echo >subdir/.mise.toml '
[tools]
tiny = "2.0.0"
[env]
FOO = "quz"
'

mise activate --status fish | source
__mise_env_eval

rtx-tiny | grep "v3.1.0"
or exit

cd subdir && __mise_env_eval
rtx-tiny | grep "v2.0.0"
or exit

cd .. && __mise_env_eval
rtx-tiny | grep "v3.1.0"
or exit

mise shell tiny@3.0.0 && __mise_env_eval
rtx-tiny | grep "v3.0.0"
or exit

mise deactivate
