#!/usr/bin/env fish

set -g mise_fish_mode disable_arrow
mise activate fish | source

set -g mise_env_eval_count 0
function __mise_env_eval
    set -g mise_env_eval_count (math $mise_env_eval_count + 1)
end

set -l activate_pwd $PWD
cd ..
__mise_env_eval_on_prompt
test $mise_env_eval_count -eq 1
or begin
    echo "expected a directory change before the first fish_prompt event to run hook-env"
    exit 1
end
cd $activate_pwd

mise deactivate
mise activate fish | source

set -g mise_env_eval_count 0
function __mise_env_eval
    set -g mise_env_eval_count (math $mise_env_eval_count + 1)
end

__mise_env_eval_on_prompt
test $mise_env_eval_count -eq 0
or begin
    echo "expected the first fish_prompt event to skip hook-env"
    exit 1
end

__mise_env_eval_on_prompt
test $mise_env_eval_count -eq 1
or begin
    echo "expected the second fish_prompt event to run hook-env"
    exit 1
end

mise deactivate
