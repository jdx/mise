#compdef rtx
_rtx() {
  typeset -A opt_args
  local context state line curcontext=$curcontext
  local ret=1

  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (activate) __rtx_activate_cmd && ret=0 ;;
        (a|aliases|alias) __rtx_alias_cmd && ret=0 ;;
        (asdf) __rtx_asdf_cmd && ret=0 ;;
        (bin-paths) __rtx_bin_paths_cmd && ret=0 ;;
        (cache) __rtx_cache_cmd && ret=0 ;;
        (complete|completions|completion) __rtx_completion_cmd && ret=0 ;;
        (cfg|config) __rtx_config_cmd && ret=0 ;;
        (current) __rtx_current_cmd && ret=0 ;;
        (deactivate) __rtx_deactivate_cmd && ret=0 ;;
        (direnv) __rtx_direnv_cmd && ret=0 ;;
        (doctor) __rtx_doctor_cmd && ret=0 ;;
        (e|env) __rtx_env_cmd && ret=0 ;;
        (env-vars) __rtx_env_vars_cmd && ret=0 ;;
        (x|exec) __rtx_exec_cmd && ret=0 ;;
        (g|global) __rtx_global_cmd && ret=0 ;;
        (hook-env) __rtx_hook_env_cmd && ret=0 ;;
        (hook-not-found) __rtx_hook_not_found_cmd && ret=0 ;;
        (implode) __rtx_implode_cmd && ret=0 ;;
        (i|install) __rtx_install_cmd && ret=0 ;;
        (latest) __rtx_latest_cmd && ret=0 ;;
        (ln|link) __rtx_link_cmd && ret=0 ;;
        (l|local) __rtx_local_cmd && ret=0 ;;
        (list|ls) __rtx_ls_cmd && ret=0 ;;
        (list-all|list-remote|ls-remote) __rtx_ls_remote_cmd && ret=0 ;;
        (outdated) __rtx_outdated_cmd && ret=0 ;;
        (p|plugin|plugin-list|plugins) __rtx_plugins_cmd && ret=0 ;;
        (prune) __rtx_prune_cmd && ret=0 ;;
        (reshim) __rtx_reshim_cmd && ret=0 ;;
        (r|run) __rtx_run_cmd && ret=0 ;;
        (self-update) __rtx_self_update_cmd && ret=0 ;;
        (settings) __rtx_settings_cmd && ret=0 ;;
        (sh|shell) __rtx_shell_cmd && ret=0 ;;
        (sync) __rtx_sync_cmd && ret=0 ;;
        (t|tasks|task) __rtx_task_cmd && ret=0 ;;
        (trust) __rtx_trust_cmd && ret=0 ;;
        (remove|rm|uninstall) __rtx_uninstall_cmd && ret=0 ;;
        (up|upgrade) __rtx_upgrade_cmd && ret=0 ;;
        (u|use) __rtx_use_cmd && ret=0 ;;
        (v|version) __rtx_version_cmd && ret=0 ;;
        (w|watch) __rtx_watch_cmd && ret=0 ;;
        (where) __rtx_where_cmd && ret=0 ;;
        (which) __rtx_which_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_activate_cmd] )) ||
__rtx_activate_cmd() {
  _arguments -s -S \
    '::shell_type:(bash fish nu xonsh zsh)' \
    '--status[Show "rtx\: <PLUGIN>@<VERSION>" message when changing directories]' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_alias_cmd] )) ||
__rtx_alias_cmd() {
  _arguments -s -S \
    '(-p --plugin)'{-p,--plugin}'=[filter aliases by plugin]:plugin:__rtx_plugins' \
    '--no-header[Don'\''t show table header]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_alias_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (get) __rtx_alias_get_cmd && ret=0 ;;
        (list|ls) __rtx_alias_ls_cmd && ret=0 ;;
        (add|create|set) __rtx_alias_set_cmd && ret=0 ;;
        (del|delete|remove|rm|unset) __rtx_alias_unset_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_alias_get_cmd] )) ||
__rtx_alias_get_cmd() {
  _arguments -s -S \
    ':plugin:__rtx_plugins' \
    ':alias:__rtx_aliases' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_alias_ls_cmd] )) ||
__rtx_alias_ls_cmd() {
  _arguments -s -S \
    '::plugin:__rtx_plugins' \
    '--no-header[Don'\''t show table header]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_alias_set_cmd] )) ||
__rtx_alias_set_cmd() {
  _arguments -s -S \
    ':plugin:__rtx_plugins' \
    ':alias:__rtx_aliases' \
    ':value:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_alias_unset_cmd] )) ||
__rtx_alias_unset_cmd() {
  _arguments -s -S \
    ':plugin:__rtx_plugins' \
    ':alias:__rtx_aliases' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_asdf_cmd] )) ||
__rtx_asdf_cmd() {
  _arguments -s -S \
    '*::args:_cmdambivalent' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_bin_paths_cmd] )) ||
__rtx_bin_paths_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_cache_cmd] )) ||
__rtx_cache_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_cache_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (c|clean|clear) __rtx_cache_clear_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_cache_clear_cmd] )) ||
__rtx_cache_clear_cmd() {
  _arguments -s -S \
    '*::plugin:__rtx_plugins' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_completion_cmd] )) ||
__rtx_completion_cmd() {
  _arguments -s -S \
    '::shell:(bash fish zsh)' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_config_cmd] )) ||
__rtx_config_cmd() {
  _arguments -s -S \
    '--no-header[Do not print table header]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_config_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (g|generate) __rtx_config_generate_cmd && ret=0 ;;
        (ls) __rtx_config_ls_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_config_generate_cmd] )) ||
__rtx_config_generate_cmd() {
  _arguments -s -S \
    '(-o --output)'{-o,--output}'=[Output to file instead of stdout]:output:_files' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_config_ls_cmd] )) ||
__rtx_config_ls_cmd() {
  _arguments -s -S \
    '--no-header[Do not print table header]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_current_cmd] )) ||
__rtx_current_cmd() {
  _arguments -s -S \
    '::plugin:__rtx_plugins' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_deactivate_cmd] )) ||
__rtx_deactivate_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_direnv_cmd] )) ||
__rtx_direnv_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_direnv_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (activate) __rtx_direnv_activate_cmd && ret=0 ;;
        (envrc) __rtx_direnv_envrc_cmd && ret=0 ;;
        (exec) __rtx_direnv_exec_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_direnv_activate_cmd] )) ||
__rtx_direnv_activate_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_direnv_envrc_cmd] )) ||
__rtx_direnv_envrc_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_direnv_exec_cmd] )) ||
__rtx_direnv_exec_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_doctor_cmd] )) ||
__rtx_doctor_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_env_cmd] )) ||
__rtx_env_cmd() {
  _arguments -s -S \
    '(-s --shell)'{-s,--shell}'=[Shell type to generate environment variables for]:shell:(bash fish nu xonsh zsh)' \
    '*::tool:__rtx_tool_versions' \
    '(-J --json)'{-J,--json}'[Output in JSON format]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_env_vars_cmd] )) ||
__rtx_env_vars_cmd() {
  _arguments -s -S \
    '--file=[The TOML file to update]:file:_files' \
    '*--remove=[Remove the environment variable from config file]:remove:' \
    '*::env_vars:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_exec_cmd] )) ||
__rtx_exec_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-c --command)'{-c,--command}'=[Command string to execute]:c:_cmdstring' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '--raw[Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_global_cmd] )) ||
__rtx_global_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '--pin[Save exact version to \`~/.tool-versions\`]' \
    '--fuzzy[Save fuzzy version to \`~/.tool-versions\`]' \
    '*--remove=[Remove the plugin(s) from ~/.tool-versions]:remove:' \
    '--path[Get the path of the global config file]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_hook_env_cmd] )) ||
__rtx_hook_env_cmd() {
  _arguments -s -S \
    '(-s --shell)'{-s,--shell}'=[Shell type to generate script for]:shell:(bash fish nu xonsh zsh)' \
    '--status[Show "rtx\: <PLUGIN>@<VERSION>" message when changing directories]' \
    '(-q --quiet)'{-q,--quiet}'[Hide warnings such as when a tool is not installed]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_hook_not_found_cmd] )) ||
__rtx_hook_not_found_cmd() {
  _arguments -s -S \
    '(-s --shell)'{-s,--shell}'=[Shell type to generate script for]:shell:(bash fish nu xonsh zsh)' \
    ':bin:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_implode_cmd] )) ||
__rtx_implode_cmd() {
  _arguments -s -S \
    '--config[Also remove config directory]' \
    '(-n --dry-run)'{-n,--dry-run}'[List directories that would be removed without actually removing them]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_install_cmd] )) ||
__rtx_install_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-f --force)'{-f,--force}'[Force reinstall even if already installed]' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '--raw[Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1]' \
    '*'{-v,--verbose}'[Show installation output]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_latest_cmd] )) ||
__rtx_latest_cmd() {
  _arguments -s -S \
    ':tool:__rtx_tool_versions' \
    '(-i --installed)'{-i,--installed}'[Show latest installed instead of available version]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_link_cmd] )) ||
__rtx_link_cmd() {
  _arguments -s -S \
    ':tool:__rtx_tool_versions' \
    ':path:_directories' \
    '(-f --force)'{-f,--force}'[Overwrite an existing tool version if it exists]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_local_cmd] )) ||
__rtx_local_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-p --parent)'{-p,--parent}'[Recurse up to find a .tool-versions file rather than using the current directory only]' \
    '--pin[Save exact version to \`.tool-versions\`]' \
    '--fuzzy[Save fuzzy version to \`.tool-versions\` e.g.\: \`rtx local --fuzzy node@20\` will save \`node 20\` to .tool-versions This is the default behavior unless RTX_ASDF_COMPAT=1]' \
    '*--remove=[Remove the plugin(s) from .tool-versions]:remove:' \
    '--path[Get the path of the config file]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_ls_cmd] )) ||
__rtx_ls_cmd() {
  _arguments -s -S \
    '*::plugin:__rtx_plugins' \
    '(-c --current)'{-c,--current}'[Only show tool versions currently specified in a .tool-versions/.rtx.toml]' \
    '(-g --global)'{-g,--global}'[Only show tool versions currently specified in a the global .tool-versions/.rtx.toml]' \
    '(-i --installed)'{-i,--installed}'[Only show tool versions that are installed Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed]' \
    '(-J --json)'{-J,--json}'[Output in json format]' \
    '(-m --missing)'{-m,--missing}'[Display missing tool versions]' \
    '--prefix=[Display versions matching this prefix]:prefix:__rtx_prefixes' \
    '--no-header[Don'\''t display headers]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_ls_remote_cmd] )) ||
__rtx_ls_remote_cmd() {
  _arguments -s -S \
    '::plugin:__rtx_plugins' \
    '--all[Show all installed plugins and versions]' \
    '::prefix:__rtx_prefixes' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_outdated_cmd] )) ||
__rtx_outdated_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_cmd] )) ||
__rtx_plugins_cmd() {
  _arguments -s -S \
    '(-c --core)'{-c,--core}'[The built-in plugins only]' \
    '--user[List installed plugins]' \
    '(-u --urls)'{-u,--urls}'[Show the git url for each plugin]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_plugins_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (a|add|i|install) __rtx_plugins_install_cmd && ret=0 ;;
        (ln|link) __rtx_plugins_link_cmd && ret=0 ;;
        (list|ls) __rtx_plugins_ls_cmd && ret=0 ;;
        (list-all|list-remote|ls-remote) __rtx_plugins_ls_remote_cmd && ret=0 ;;
        (remove|rm|uninstall) __rtx_plugins_uninstall_cmd && ret=0 ;;
        (upgrade|update) __rtx_plugins_update_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_plugins_install_cmd] )) ||
__rtx_plugins_install_cmd() {
  _arguments -s -S \
    ':new_plugin:__rtx_all_plugins' \
    '::git_url:_urls' \
    '(-f --force)'{-f,--force}'[Reinstall even if plugin exists]' \
    '(-a --all)'{-a,--all}'[Install all missing plugins]' \
    '*'{-v,--verbose}'[Show installation output]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_link_cmd] )) ||
__rtx_plugins_link_cmd() {
  _arguments -s -S \
    ':name:' \
    '::path:_directories' \
    '(-f --force)'{-f,--force}'[Overwrite existing plugin]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_ls_cmd] )) ||
__rtx_plugins_ls_cmd() {
  _arguments -s -S \
    '(-c --core)'{-c,--core}'[The built-in plugins only]' \
    '--user[List installed plugins]' \
    '(-u --urls)'{-u,--urls}'[Show the git url for each plugin]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_ls_remote_cmd] )) ||
__rtx_plugins_ls_remote_cmd() {
  _arguments -s -S \
    '(-u --urls)'{-u,--urls}'[Show the git url for each plugin e.g.\: https\://github.com/rtx-plugins/rtx-nodejs.git]' \
    '--only-names[Only show the name of each plugin by default it will show a "*" next to installed plugins]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_uninstall_cmd] )) ||
__rtx_plugins_uninstall_cmd() {
  _arguments -s -S \
    '*::plugin:__rtx_plugins' \
    '(-p --purge)'{-p,--purge}'[Also remove the plugin'\''s installs, downloads, and cache]' \
    '(-a --all)'{-a,--all}'[Remove all plugins]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_plugins_update_cmd] )) ||
__rtx_plugins_update_cmd() {
  _arguments -s -S \
    '*::plugin:__rtx_plugins' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_prune_cmd] )) ||
__rtx_prune_cmd() {
  _arguments -s -S \
    '*::plugin:__rtx_plugins' \
    '(-n --dry-run)'{-n,--dry-run}'[Do not actually delete anything]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_reshim_cmd] )) ||
__rtx_reshim_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_run_cmd] )) ||
__rtx_run_cmd() {
  _arguments -s -S \
    '::task:__rtx_tasks' \
    '*::args:' \
    '(-C --cd)'{-C,--cd}'=[Change to this directory before executing the command]:cd:_directories' \
    '(-n --dry-run)'{-n,--dry-run}'[Don'\''t actually run the task(s), just print them in order of execution]' \
    '(-f --force)'{-f,--force}'[Force the task to run even if outputs are up to date]' \
    '(-p --prefix)'{-p,--prefix}'[Print stdout/stderr by line, prefixed with the task'\''s label]' \
    '(-i --interleave)'{-i,--interleave}'[Print directly to stdout/stderr instead of by line]' \
    '*'{-t,--tool}'=[Tool(s) to also add e.g.\: node@20 python@3.10]:tool:__rtx_tool_versions' \
    '(-j --jobs)'{-j,--jobs}'=[Number of tasks to run in parallel]:jobs:' \
    '(-r --raw)'{-r,--raw}'[Read/write directly to stdin/stdout/stderr instead of by line]' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_self_update_cmd] )) ||
__rtx_self_update_cmd() {
  _arguments -s -S \
    '(-f --force)'{-f,--force}'[Update even if already up to date]' \
    '--no-plugins[Disable auto-updating plugins]' \
    '(-y --yes)'{-y,--yes}'[Skip confirmation prompt]' \
    '::version:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]'
}
(( $+functions[__rtx_settings_cmd] )) ||
__rtx_settings_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_settings_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (get) __rtx_settings_get_cmd && ret=0 ;;
        (list|ls) __rtx_settings_ls_cmd && ret=0 ;;
        (add|create|set) __rtx_settings_set_cmd && ret=0 ;;
        (del|delete|remove|rm|unset) __rtx_settings_unset_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_settings_get_cmd] )) ||
__rtx_settings_get_cmd() {
  _arguments -s -S \
    ':setting:__rtx_settings' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_settings_ls_cmd] )) ||
__rtx_settings_ls_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_settings_set_cmd] )) ||
__rtx_settings_set_cmd() {
  _arguments -s -S \
    ':setting:__rtx_settings' \
    ':value:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_settings_unset_cmd] )) ||
__rtx_settings_unset_cmd() {
  _arguments -s -S \
    ':setting:__rtx_settings' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_shell_cmd] )) ||
__rtx_shell_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '--raw[Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1]' \
    '(-u --unset)'{-u,--unset}'[Removes a previously set version]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_sync_cmd] )) ||
__rtx_sync_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_sync_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (node) __rtx_sync_node_cmd && ret=0 ;;
        (python) __rtx_sync_python_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_sync_node_cmd] )) ||
__rtx_sync_node_cmd() {
  _arguments -s -S \
    '--brew[Get tool versions from Homebrew]' \
    '--nvm[Get tool versions from nvm]' \
    '--nodenv[Get tool versions from nodenv]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_sync_python_cmd] )) ||
__rtx_sync_python_cmd() {
  _arguments -s -S \
    '--pyenv[Get tool versions from pyenv]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_task_cmd] )) ||
__rtx_task_cmd() {
  _arguments -s -S \
    '--no-header[Do not print table header]' \
    '--hidden[Show hidden tasks]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]' \
    '1: :__rtx_task_cmds' \
    '*::arg:->args' && ret=0

      case "$state" in
    (args)
      curcontext="${curcontext%:*:*}:rtx-cmd-$words[1]:"
      case $words[1] in
        (edit) __rtx_task_edit_cmd && ret=0 ;;
        (ls) __rtx_task_ls_cmd && ret=0 ;;
        (r|run) __rtx_task_run_cmd && ret=0 ;;
      esac
    ;;
  esac

return ret
}
(( $+functions[__rtx_task_edit_cmd] )) ||
__rtx_task_edit_cmd() {
  _arguments -s -S \
    ':task:__rtx_tasks' \
    '(-p --path)'{-p,--path}'[Display the path to the task instead of editing it]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_task_ls_cmd] )) ||
__rtx_task_ls_cmd() {
  _arguments -s -S \
    '--no-header[Do not print table header]' \
    '--hidden[Show hidden tasks]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_task_run_cmd] )) ||
__rtx_task_run_cmd() {
  _arguments -s -S \
    '::task:__rtx_tasks' \
    '*::args:' \
    '(-C --cd)'{-C,--cd}'=[Change to this directory before executing the command]:cd:_directories' \
    '(-n --dry-run)'{-n,--dry-run}'[Don'\''t actually run the task(s), just print them in order of execution]' \
    '(-f --force)'{-f,--force}'[Force the task to run even if outputs are up to date]' \
    '(-p --prefix)'{-p,--prefix}'[Print stdout/stderr by line, prefixed with the task'\''s label]' \
    '(-i --interleave)'{-i,--interleave}'[Print directly to stdout/stderr instead of by line]' \
    '*'{-t,--tool}'=[Tool(s) to also add e.g.\: node@20 python@3.10]:tool:__rtx_tool_versions' \
    '(-j --jobs)'{-j,--jobs}'=[Number of tasks to run in parallel]:jobs:' \
    '(-r --raw)'{-r,--raw}'[Read/write directly to stdin/stdout/stderr instead of by line]' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_trust_cmd] )) ||
__rtx_trust_cmd() {
  _arguments -s -S \
    '::config_file:_files' \
    '(-a --all)'{-a,--all}'[Trust all config files in the current directory and its parents]' \
    '--untrust[No longer trust this config]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_uninstall_cmd] )) ||
__rtx_uninstall_cmd() {
  _arguments -s -S \
    '*::installed_tool:__rtx_installed_tool_versions' \
    '(-a --all)'{-a,--all}'[Delete all installed versions]' \
    '(-n --dry-run)'{-n,--dry-run}'[Do not actually delete anything]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_upgrade_cmd] )) ||
__rtx_upgrade_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-n --dry-run)'{-n,--dry-run}'[Just print what would be done, don'\''t actually do it]' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '(-i --interactive)'{-i,--interactive}'[Display multiselect menu to choose which tools to upgrade]' \
    '--raw[Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_use_cmd] )) ||
__rtx_use_cmd() {
  _arguments -s -S \
    '*::tool:__rtx_tool_versions' \
    '(-f --force)'{-f,--force}'[Force reinstall even if already installed]' \
    '--fuzzy[Save fuzzy version to config file]' \
    '(-g --global)'{-g,--global}'[Use the global config file (~/.config/rtx/config.toml) instead of the local one]' \
    '(-e --env)'{-e,--env}'=[Modify an environment-specific config file like .rtx.<env>.toml]:env:' \
    '(-j --jobs)'{-j,--jobs}'=[Number of jobs to run in parallel]:jobs:' \
    '--raw[Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1]' \
    '*--remove=[Remove the tool(s) from config file]:remove:' \
    '(-p --path)'{-p,--path}'=[Specify a path to a config file or directory If a directory is specified, it will look for .rtx.toml (default) or .tool-versions]:path:_files' \
    '--pin[Save exact version to config file]' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_version_cmd] )) ||
__rtx_version_cmd() {
  _arguments -s -S \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_watch_cmd] )) ||
__rtx_watch_cmd() {
  _arguments -s -S \
    '*'{-t,--task}'=[Task to run]:task:__rtx_tasks' \
    '*::args:' \
    '*'{-g,--glob}'=[Files to watch]:glob:' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_where_cmd] )) ||
__rtx_where_cmd() {
  _arguments -s -S \
    ':tool:__rtx_tool_versions' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_which_cmd] )) ||
__rtx_which_cmd() {
  _arguments -s -S \
    ':bin_name:' \
    '--plugin[Show the plugin name instead of the path]' \
    '--version[Show the version instead of the path]' \
    '(-t --tool)'{-t,--tool}'=[Use a specific tool@version]:tool:__rtx_tool_versions' \
    '(-C --cd)'{-C,--cd}'=[Change directory before running command]:cd:_directories' \
    '(-q --quiet)'{-q,--quiet}'[Suppress non-error messages]' \
    '*'{-v,--verbose}'[Show extra output (use -vv for even more)]' \
    '(-y --yes)'{-y,--yes}'[Answer yes to all confirmation prompts]'
}
(( $+functions[__rtx_cmds] )) ||
__rtx_cmds() {
  local commands; commands=(
    'activate:Initializes rtx in the current shell session'
    {a,alias}':Manage aliases'
    'bin-paths:List all the active runtime bin paths'
    'cache:Manage the rtx cache'
    'completion:Generate shell completions'
    {cfg,config}':\[experimental\] Manage config files'
    'current:Shows current active and installed runtime versions'
    'deactivate:Disable rtx for current shell session'
    'direnv:Output direnv function to use rtx inside direnv'
    'doctor:Check rtx installation for possible problems.'
    {e,env}':Exports env vars to activate rtx a single time'
    'env-vars:Manage environment variables'
    {x,exec}':Execute a command with tool(s) set'
    'implode:Removes rtx CLI and all related data'
    {i,install}':Install a tool version'
    'latest:Gets the latest available version for a plugin'
    {ln,link}':Symlinks a tool version into rtx'
    {list,ls}':List installed and/or currently selected tool versions'
    'ls-remote:List runtime versions available for install'
    'outdated:Shows outdated tool versions'
    {p,plugins}':Manage plugins'
    'prune:Delete unused versions of tools'
    'reshim:rebuilds the shim farm'
    {r,run}':\[experimental\] Run a task'
    'self-update:Updates rtx itself'
    'settings:Manage settings'
    {sh,shell}':Sets a tool version for the current shell session'
    'sync:Add tool versions from external tools to rtx'
    {t,task}':\[experimental\] Manage tasks'
    'trust:Marks a config file as trusted'
    {remove,rm,uninstall}':Removes runtime versions'
    {up,upgrade}':Upgrades outdated tool versions'
    {u,use}':Change the active version of a tool locally or globally.'
    'version:Show rtx version'
    {w,watch}':\[experimental\] Run a task watching for changes'
    'where:Display the installation path for a runtime'
    'which:Shows the path that a bin name points to'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_alias_cmds] )) ||
__rtx_alias_cmds() {
  local commands; commands=(
    'get:Show an alias for a plugin'
    {list,ls}':List aliases'
    {add,create,set}':Add/update an alias for a plugin'
    {del,delete,remove,rm,unset}':Clears an alias for a plugin'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_cache_cmds] )) ||
__rtx_cache_cmds() {
  local commands; commands=(
    {c,clear}':Deletes all cache files in rtx'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_config_cmds] )) ||
__rtx_config_cmds() {
  local commands; commands=(
    {g,generate}':\[experimental\] Generate an .rtx.toml file'
    'ls:\[experimental\] List config files currently in use'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_direnv_cmds] )) ||
__rtx_direnv_cmds() {
  local commands; commands=(
    'activate:Output direnv function to use rtx inside direnv'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_plugins_cmds] )) ||
__rtx_plugins_cmds() {
  local commands; commands=(
    {a,add,i,install}':Install a plugin'
    {ln,link}':Symlinks a plugin into rtx'
    {list,ls}':List installed plugins'
    {list-all,list-remote,ls-remote}':List all available remote plugins'
    {remove,rm,uninstall}':Removes a plugin'
    {upgrade,update}':Updates a plugin to the latest version'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_settings_cmds] )) ||
__rtx_settings_cmds() {
  local commands; commands=(
    'get:Show a current setting'
    {list,ls}':Show current settings'
    {add,create,set}':Add/update a setting'
    {del,delete,remove,rm,unset}':Clears a setting'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_sync_cmds] )) ||
__rtx_sync_cmds() {
  local commands; commands=(
    'node:Symlinks all tool versions from an external tool into rtx'
    'python:Symlinks all tool versions from an external tool into rtx'
  )
  _describe -t commands 'command' commands "$@"
}
(( $+functions[__rtx_task_cmds] )) ||
__rtx_task_cmds() {
  local commands; commands=(
    'edit:\[experimental\] Edit a task with \$EDITOR'
    'ls:\[experimental\] List available tasks to execute'
    {r,run}':\[experimental\] Run a task'
  )
  _describe -t commands 'command' commands "$@"
}

(( $+functions[__rtx_tool_versions] )) ||
__rtx_tool_versions() {
  if compset -P '*@'; then
    local -a tool_versions; tool_versions=($(rtx ls-remote ${words[CURRENT]}))
    _wanted tool_version expl 'version of tool' \
      compadd -a tool_versions -o nosort
  else
    local -a plugins; plugins=($(rtx plugins --core --user))
    _wanted plugin expl 'plugin name' \
      compadd -S '@' -a plugins
  fi
}
(( $+functions[__rtx_installed_tool_versions] )) ||
__rtx_installed_tool_versions() {
  if compset -P '*@'; then
    local plugin; plugin=${words[CURRENT]%%@*}
    local -a installed_tool_versions; installed_tool_versions=($(rtx ls --installed $plugin | awk '{print $2}'))
    _wanted installed_tool_version expl 'version of tool' \
      compadd -a installed_tool_versions -o nosort
  else
    local -a plugins; plugins=($(rtx plugins --core --user))
    _wanted plugin expl 'plugin name' \
      compadd -S '@' -a plugins
  fi
}
(( $+functions[__rtx_plugins] )) ||
__rtx_plugins() {
  local -a plugins; plugins=($(rtx plugins --core --user))
  _describe -t plugins 'plugin' plugins "$@"
}
(( $+functions[__rtx_all_plugins] )) ||
__rtx_all_plugins() {
  local -a all_plugins; all_plugins=($(rtx plugins --all))
  _describe -t all_plugins 'all_plugins' all_plugins "$@"
}
(( $+functions[__rtx_aliases] )) ||
__rtx_aliases() {
  local -a aliases; aliases=($(rtx aliases ls ${words[CURRENT-1]} | awk '{print $2}'))
  _describe -t aliases 'alias' aliases "$@"
}
(( $+functions[__rtx_settings] )) ||
__rtx_settings() {
  local -a settings; settings=($(rtx settings ls | awk '{print $1}'))
  _describe -t settings 'setting' settings "$@"
}
(( $+functions[__rtx_tasks] )) ||
__rtx_tasks() {
  local -a tasks; tasks=($(rtx tasks ls --no-header | awk '{print $1}'))
  _describe -t tasks 'task' tasks "$@"
}
(( $+functions[__rtx_prefixes] )) ||
__rtx_prefixes() {
  if [[ CURRENT -gt 2 ]]; then
      local -a prefixes; prefixes=($(rtx ls-remote ${words[CURRENT-1]}))
      _describe -t prefixes 'prefix' prefixes "$@"
  fi
}

if [ "$funcstack[1]" = "_rtx" ]; then
    _rtx "$@"
else
    compdef _rtx rtx
fi

# vim: noet ci pi sts=0 sw=4 ts=4
