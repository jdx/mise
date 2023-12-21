set -l fssf "__fish_seen_subcommand_from"

# rtx
complete -kxc rtx -s q -l quiet -d 'Suppress warnings and other non-error messages'
complete -kxc rtx -s v -l verbose -d 'Show extra output (use -vv for even more)'
complete -kxc rtx -s y -l yes -d 'Answer yes to all prompts'
set -l others activate alias bin-paths cache completion config current deactivate direnv doctor env env-vars exec implode install latest link ls ls-remote outdated plugins prune reshim self-update settings shell sync trust uninstall upgrade use version where which
complete -xc rtx -n "not $fssf $others" -a activate -d 'Initializes rtx in the current shell session'
complete -xc rtx -n "not $fssf $others" -a alias -d 'Manage aliases'
complete -xc rtx -n "not $fssf $others" -a bin-paths -d 'List all the active runtime bin paths'
complete -xc rtx -n "not $fssf $others" -a cache -d 'Manage the rtx cache'
complete -xc rtx -n "not $fssf $others" -a completion -d 'Generate shell completions'
complete -xc rtx -n "not $fssf $others" -a config -d '[experimental] Manage config files'
complete -xc rtx -n "not $fssf $others" -a current -d 'Shows current active and installed runtime versions'
complete -xc rtx -n "not $fssf $others" -a deactivate -d 'Disable rtx for current shell session'
complete -xc rtx -n "not $fssf $others" -a direnv -d 'Output direnv function to use rtx inside direnv'
complete -xc rtx -n "not $fssf $others" -a doctor -d 'Check rtx installation for possible problems.'
complete -xc rtx -n "not $fssf $others" -a env -d 'Exports env vars to activate rtx a single time'
complete -xc rtx -n "not $fssf $others" -a env-vars -d 'Manage environment variables'
complete -xc rtx -n "not $fssf $others" -a exec -d 'Execute a command with tool(s) set'
complete -xc rtx -n "not $fssf $others" -a implode -d 'Removes rtx CLI and all related data'
complete -xc rtx -n "not $fssf $others" -a install -d 'Install a tool version'
complete -xc rtx -n "not $fssf $others" -a latest -d 'Gets the latest available version for a plugin'
complete -xc rtx -n "not $fssf $others" -a link -d 'Symlinks a tool version into rtx'
complete -xc rtx -n "not $fssf $others" -a ls -d 'List installed and/or currently selected tool versions'
complete -xc rtx -n "not $fssf $others" -a ls-remote -d 'List runtime versions available for install'
complete -xc rtx -n "not $fssf $others" -a outdated -d 'Shows outdated tool versions'
complete -xc rtx -n "not $fssf $others" -a plugins -d 'Manage plugins'
complete -xc rtx -n "not $fssf $others" -a prune -d 'Delete unused versions of tools'
complete -xc rtx -n "not $fssf $others" -a reshim -d 'rebuilds the shim farm'
complete -xc rtx -n "not $fssf $others" -a self-update -d 'Updates rtx itself'
complete -xc rtx -n "not $fssf $others" -a settings -d 'Manage settings'
complete -xc rtx -n "not $fssf $others" -a shell -d 'Sets a tool version for the current shell session'
complete -xc rtx -n "not $fssf $others" -a sync -d 'Add tool versions from external tools to rtx'
complete -xc rtx -n "not $fssf $others" -a trust -d 'Marks a config file as trusted'
complete -xc rtx -n "not $fssf $others" -a uninstall -d 'Removes runtime versions'
complete -xc rtx -n "not $fssf $others" -a upgrade -d 'Upgrades outdated tool versions'
complete -xc rtx -n "not $fssf $others" -a use -d 'Change the active version of a tool locally or globally.'
complete -xc rtx -n "not $fssf $others" -a version -d 'Show rtx version'
complete -xc rtx -n "not $fssf $others" -a where -d 'Display the installation path for a runtime'
complete -xc rtx -n "not $fssf $others" -a which -d 'Shows the path that a bin name points to'

# activate
complete -kxc rtx -n "$fssf activate" -s q -l quiet -d 'Hide warnings such as when a tool is not installed'
complete -kxc rtx -n "$fssf activate" -a "bash fish nu xonsh zsh" -d 'Shell type to generate the script for'
complete -kxc rtx -n "$fssf activate" -l status -d 'Show "rtx: <PLUGIN>@<VERSION>" message when changing directories'

# alias
complete -kxc rtx -n "$fssf alias" -l no-header -d 'Don'\''t show table header'
complete -kxc rtx -n "$fssf alias" -s p -l plugin -a "(__rtx_plugins)" -d 'filter aliases by plugin'
set -l others get ls set unset
complete -xc rtx -n "$fssf alias; and not $fssf $others" -a get -d 'Show an alias for a plugin'
complete -xc rtx -n "$fssf alias; and not $fssf $others" -a ls -d 'List aliases'
complete -xc rtx -n "$fssf alias; and not $fssf $others" -a set -d 'Add/update an alias for a plugin'
complete -xc rtx -n "$fssf alias; and not $fssf $others" -a unset -d 'Clears an alias for a plugin'

# alias get
complete -kxc rtx -n "$fssf alias; and $fssf get" -a "(__rtx_aliases)" -d 'The alias to show'
complete -kxc rtx -n "$fssf alias; and $fssf get" -a "(__rtx_plugins)" -d 'The plugin to show the alias for'

# alias ls
complete -kxc rtx -n "$fssf alias; and $fssf ls" -l no-header -d 'Don'\''t show table header'
complete -kxc rtx -n "$fssf alias; and $fssf ls" -a "(__rtx_plugins)" -d 'Show aliases for <PLUGIN>'

# alias set
complete -kxc rtx -n "$fssf alias; and $fssf set" -a "(__rtx_aliases)" -d 'The alias to set'
complete -kxc rtx -n "$fssf alias; and $fssf set" -a "(__rtx_plugins)" -d 'The plugin to set the alias for'
complete -kxc rtx -n "$fssf alias; and $fssf set" -d 'The value to set the alias to'

# alias unset
complete -kxc rtx -n "$fssf alias; and $fssf unset" -a "(__rtx_aliases)" -d 'The alias to remove'
complete -kxc rtx -n "$fssf alias; and $fssf unset" -a "(__rtx_plugins)" -d 'The plugin to remove the alias from'


# bin-paths

# cache
set -l others clear
complete -xc rtx -n "$fssf cache; and not $fssf $others" -a clear -d 'Deletes all cache files in rtx'

# cache clear
complete -kxc rtx -n "$fssf cache; and $fssf clear" -a "(__rtx_plugins)" -d 'Plugin(s) to clear cache for e.g.: node, python'


# completion
complete -kxc rtx -n "$fssf completion" -a "bash fish zsh" -d 'Shell type to generate completions for'

# config
complete -kxc rtx -n "$fssf config" -l no-header -d 'Do not print table header'
set -l others generate ls
complete -xc rtx -n "$fssf config; and not $fssf $others" -a generate -d '[experimental] Generate an .rtx.toml file'
complete -xc rtx -n "$fssf config; and not $fssf $others" -a ls -d '[experimental] List config files currently in use'

# config generate
complete -kxc rtx -n "$fssf config; and $fssf generate" -s o -l output -a "(__fish_complete_path)" -d 'Output to file instead of stdout'

# config ls
complete -kxc rtx -n "$fssf config; and $fssf ls" -l no-header -d 'Do not print table header'


# current
complete -kxc rtx -n "$fssf current" -a "(__rtx_plugins)" -d 'Plugin to show versions of e.g.: ruby, node'

# deactivate

# direnv
set -l others activate
complete -xc rtx -n "$fssf direnv; and not $fssf $others" -a activate -d 'Output direnv function to use rtx inside direnv'

# direnv activate


# doctor

# env
complete -kxc rtx -n "$fssf env" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf env" -s J -l json -d 'Output in JSON format'
complete -kxc rtx -n "$fssf env" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf env" -s s -l shell -a "bash fish nu xonsh zsh" -d 'Shell type to generate environment variables for'
complete -kxc rtx -n "$fssf env" -a "(__rtx_tool_versions)" -d 'Tool(s) to use'

# env-vars
complete -kxc rtx -n "$fssf env-vars" -d 'Environment variable(s) to set'
complete -kxc rtx -n "$fssf env-vars" -l file -a "(__fish_complete_path)" -d 'The TOML file to update'
complete -kxc rtx -n "$fssf env-vars" -l remove -d 'Remove the environment variable from config file'

# exec
complete -kxc rtx -n "$fssf exec" -s c -l command -d 'Command string to execute'
complete -kxc rtx -n "$fssf exec" -s C -l cd -a "(__fish_complete_directories)" -d 'Change to this directory before executing the command'
complete -kxc rtx -n "$fssf exec" -d 'Command string to execute (same as --command)'
complete -kxc rtx -n "$fssf exec" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf exec" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf exec" -a "(__rtx_tool_versions)" -d 'Tool(s) to start e.g.: node@20 python@3.10'

# implode
complete -kxc rtx -n "$fssf implode" -l config -d 'Also remove config directory'
complete -kxc rtx -n "$fssf implode" -s n -l dry-run -d 'List directories that would be removed without actually removing them'

# install
complete -kxc rtx -n "$fssf install" -s f -l force -d 'Force reinstall even if already installed'
complete -kxc rtx -n "$fssf install" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf install" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf install" -a "(__rtx_tool_versions)" -d 'Tool(s) to install e.g.: node@20'
complete -kxc rtx -n "$fssf install" -s v -l verbose -d 'Show installation output'

# latest
complete -kxc rtx -n "$fssf latest" -s i -l installed -d 'Show latest installed instead of available version'
complete -kxc rtx -n "$fssf latest" -a "(__rtx_tool_versions)" -d 'Tool to get the latest version of'

# link
complete -kxc rtx -n "$fssf link" -s f -l force -d 'Overwrite an existing tool version if it exists'
complete -kxc rtx -n "$fssf link" -a "(__fish_complete_directories)" -d 'The local path to the tool version'
complete -kxc rtx -n "$fssf link" -a "(__rtx_tool_versions)" -d 'Tool name and version to create a symlink for'

# ls
complete -kxc rtx -n "$fssf ls" -s c -l current -d 'Only show tool versions currently specified in a .tool-versions/.rtx.toml'
complete -kxc rtx -n "$fssf ls" -s g -l global -d 'Only show tool versions currently specified in a the global .tool-versions/.rtx.toml'
complete -kxc rtx -n "$fssf ls" -s i -l installed -d 'Only show tool versions that are installed Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed'
complete -kxc rtx -n "$fssf ls" -s J -l json -d 'Output in json format'
complete -kxc rtx -n "$fssf ls" -s m -l missing -d 'Display missing tool versions'
complete -kxc rtx -n "$fssf ls" -l no-header -d 'Don'\''t display headers'
complete -kxc rtx -n "$fssf ls" -a "(__rtx_plugins)" -d 'Only show tool versions from [PLUGIN]'
complete -kxc rtx -n "$fssf ls" -l prefix -d 'Display versions matching this prefix'

# ls-remote
complete -kxc rtx -n "$fssf ls-remote" -l all -d 'Show all installed plugins and versions'
complete -kxc rtx -n "$fssf ls-remote" -a "(__rtx_plugins)" -d 'Plugin to get versions for'
complete -kxc rtx -n "$fssf ls-remote" -d 'The version prefix to use when querying the latest version'

# outdated
complete -kxc rtx -n "$fssf outdated" -a "(__rtx_tool_versions)" -d 'Tool(s) to show outdated versions for'

# plugins
complete -kxc rtx -n "$fssf plugins" -s c -l core -d 'The built-in plugins only'
complete -kxc rtx -n "$fssf plugins" -s u -l urls -d 'Show the git url for each plugin'
complete -kxc rtx -n "$fssf plugins" -l user -d 'List installed plugins'
set -l others install link ls ls-remote uninstall update
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a install -d 'Install a plugin'
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a link -d 'Symlinks a plugin into rtx'
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a ls -d 'List installed plugins'
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a ls-remote -d 'List all available remote plugins'
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a uninstall -d 'Removes a plugin'
complete -xc rtx -n "$fssf plugins; and not $fssf $others" -a update -d 'Updates a plugin to the latest version'

# plugins install
complete -kxc rtx -n "$fssf plugins; and $fssf install" -s a -l all -d 'Install all missing plugins'
complete -kxc rtx -n "$fssf plugins; and $fssf install" -s f -l force -d 'Reinstall even if plugin exists'
complete -kxc rtx -n "$fssf plugins; and $fssf install" -d 'The git url of the plugin'
complete -kxc rtx -n "$fssf plugins; and $fssf install" -a "(__rtx_all_plugins)" -d 'The name of the plugin to install'
complete -kxc rtx -n "$fssf plugins; and $fssf install" -s v -l verbose -d 'Show installation output'

# plugins link
complete -kxc rtx -n "$fssf plugins; and $fssf link" -s f -l force -d 'Overwrite existing plugin'
complete -kxc rtx -n "$fssf plugins; and $fssf link" -d 'The name of the plugin'
complete -kxc rtx -n "$fssf plugins; and $fssf link" -a "(__fish_complete_directories)" -d 'The local path to the plugin'

# plugins ls
complete -kxc rtx -n "$fssf plugins; and $fssf ls" -s c -l core -d 'The built-in plugins only'
complete -kxc rtx -n "$fssf plugins; and $fssf ls" -s u -l urls -d 'Show the git url for each plugin'
complete -kxc rtx -n "$fssf plugins; and $fssf ls" -l user -d 'List installed plugins'

# plugins ls-remote
complete -kxc rtx -n "$fssf plugins; and $fssf ls-remote" -l only-names -d 'Only show the name of each plugin by default it will show a "*" next to installed plugins'
complete -kxc rtx -n "$fssf plugins; and $fssf ls-remote" -s u -l urls -d 'Show the git url for each plugin e.g.: https://github.com/rtx-plugins/rtx-nodejs.git'

# plugins uninstall
complete -kxc rtx -n "$fssf plugins; and $fssf uninstall" -s a -l all -d 'Remove all plugins'
complete -kxc rtx -n "$fssf plugins; and $fssf uninstall" -a "(__rtx_plugins)" -d 'Plugin(s) to remove'
complete -kxc rtx -n "$fssf plugins; and $fssf uninstall" -s p -l purge -d 'Also remove the plugin'\''s installs, downloads, and cache'

# plugins update
complete -kxc rtx -n "$fssf plugins; and $fssf update" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf plugins; and $fssf update" -a "(__rtx_plugins)" -d 'Plugin(s) to update'


# prune
complete -kxc rtx -n "$fssf prune" -s n -l dry-run -d 'Do not actually delete anything'
complete -kxc rtx -n "$fssf prune" -a "(__rtx_plugins)" -d 'Prune only versions from these plugins'

# reshim

# self-update
complete -kxc rtx -n "$fssf self-update" -s f -l force -d 'Update even if already up to date'
complete -kxc rtx -n "$fssf self-update" -l no-plugins -d 'Disable auto-updating plugins'
complete -kxc rtx -n "$fssf self-update" -d 'Update to a specific version'
complete -kxc rtx -n "$fssf self-update" -s y -l yes -d 'Skip confirmation prompt'

# settings
set -l others get ls set unset
complete -xc rtx -n "$fssf settings; and not $fssf $others" -a get -d 'Show a current setting'
complete -xc rtx -n "$fssf settings; and not $fssf $others" -a ls -d 'Show current settings'
complete -xc rtx -n "$fssf settings; and not $fssf $others" -a set -d 'Add/update a setting'
complete -xc rtx -n "$fssf settings; and not $fssf $others" -a unset -d 'Clears a setting'

# settings get
complete -kxc rtx -n "$fssf settings; and $fssf get" -a "(__rtx_settings)" -d 'The setting to show'

# settings ls

# settings set
complete -kxc rtx -n "$fssf settings; and $fssf set" -a "(__rtx_settings)" -d 'The setting to set'
complete -kxc rtx -n "$fssf settings; and $fssf set" -d 'The value to set'

# settings unset
complete -kxc rtx -n "$fssf settings; and $fssf unset" -a "(__rtx_settings)" -d 'The setting to remove'


# shell
complete -kxc rtx -n "$fssf shell" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf shell" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf shell" -a "(__rtx_tool_versions)" -d 'Tool(s) to use'
complete -kxc rtx -n "$fssf shell" -s u -l unset -d 'Removes a previously set version'

# sync
set -l others node python
complete -xc rtx -n "$fssf sync; and not $fssf $others" -a node -d 'Symlinks all tool versions from an external tool into rtx'
complete -xc rtx -n "$fssf sync; and not $fssf $others" -a python -d 'Symlinks all tool versions from an external tool into rtx'

# sync node
complete -kxc rtx -n "$fssf sync; and $fssf node" -l brew -d 'Get tool versions from Homebrew'
complete -kxc rtx -n "$fssf sync; and $fssf node" -l nodenv -d 'Get tool versions from nodenv'
complete -kxc rtx -n "$fssf sync; and $fssf node" -l nvm -d 'Get tool versions from nvm'

# sync python
complete -kxc rtx -n "$fssf sync; and $fssf python" -l pyenv -d 'Get tool versions from pyenv'


# trust
complete -kxc rtx -n "$fssf trust" -a "(__fish_complete_path)" -d 'The config file to trust'
complete -kxc rtx -n "$fssf trust" -l untrust -d 'No longer trust this config'

# uninstall
complete -kxc rtx -n "$fssf uninstall" -s a -l all -d 'Delete all installed versions'
complete -kxc rtx -n "$fssf uninstall" -s n -l dry-run -d 'Do not actually delete anything'
complete -kxc rtx -n "$fssf uninstall" -a "(__rtx_installed_tool_versions)" -d 'Tool(s) to remove'

# upgrade
complete -kxc rtx -n "$fssf upgrade" -s n -l dry-run -d 'Just print what would be done, don'\''t actually do it'
complete -kxc rtx -n "$fssf upgrade" -s i -l interactive -d 'Display multiselect menu to choose which tools to upgrade'
complete -kxc rtx -n "$fssf upgrade" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf upgrade" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf upgrade" -a "(__rtx_tool_versions)" -d 'Tool(s) to upgrade'

# use
complete -kxc rtx -n "$fssf use" -s e -l env -d 'Modify an environment-specific config file like .rtx.<env>.toml'
complete -kxc rtx -n "$fssf use" -s f -l force -d 'Force reinstall even if already installed'
complete -kxc rtx -n "$fssf use" -l fuzzy -d 'Save fuzzy version to config file'
complete -kxc rtx -n "$fssf use" -s g -l global -d 'Use the global config file (~/.config/rtx/config.toml) instead of the local one'
complete -kxc rtx -n "$fssf use" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc rtx -n "$fssf use" -s p -l path -a "(__fish_complete_path)" -d 'Specify a path to a config file or directory If a directory is specified, it will look for .rtx.toml (default) or .tool-versions'
complete -kxc rtx -n "$fssf use" -l pin -d 'Save exact version to config file'
complete -kxc rtx -n "$fssf use" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc rtx -n "$fssf use" -l remove -d 'Remove the tool(s) from config file'
complete -kxc rtx -n "$fssf use" -a "(__rtx_tool_versions)" -d 'Tool(s) to add to config file'

# version

# where
complete -kxc rtx -n "$fssf where" -a "(__rtx_tool_versions)" -d 'Tool(s) to look up'

# which
complete -kxc rtx -n "$fssf which" -d 'The bin name to look up'
complete -kxc rtx -n "$fssf which" -l plugin -a "(__rtx_plugins)" -d 'Show the plugin name instead of the path'
complete -kxc rtx -n "$fssf which" -s t -l tool -a "(__rtx_tool_versions)" -d 'Use a specific tool@version'
complete -kxc rtx -n "$fssf which" -l version -d 'Show the version instead of the path'



function __rtx_all_plugins
    if test -z "$__rtx_all_plugins_cache"
        set -g __rtx_all_plugins_cache (rtx plugins ls --all)
    end
    for p in $__rtx_all_plugins_cache
        echo $p
    end
end
function __rtx_plugins
    if test -z "$__rtx_plugins_cache"
        set -g __rtx_plugins_cache (rtx plugins ls --core --user)
    end
    for p in $__rtx_plugins_cache
        echo $p
    end
end
function __rtx_tool_versions
    if test -z "$__rtx_tool_versions_cache"
        set -g __rtx_tool_versions_cache (rtx plugins --core --user) (rtx ls-remote --all | tac)
    end
    for tv in $__rtx_tool_versions_cache
        echo $tv
    end
end
function __rtx_installed_tool_versions
    if test -z "$__rtx_installed_tool_versions_cache"
        set -g __rtx_installed_tool_versions_cache (rtx ls --installed | awk '{print $1 "@" $2}')
    end
    for tv in $__rtx_installed_tool_versions_cache
        echo $tv
    end
end
function __rtx_aliases
    if test -z "$__rtx_aliases_cache"
        set -g __rtx_aliases_cache (rtx alias ls | awk '{print $2}')
    end
    for a in $__rtx_aliases_cache
        echo $a
    end
end
function __rtx_settings
    if test -z "$__rtx_settings_cache"
        set -g __rtx_settings_cache (rtx settings ls | awk '{print $1}')
    end
    for s in $__rtx_settings_cache
        echo $s
    end
end

# vim: noet ci pi sts=0 sw=4 ts=4
