set -l fssf "__fish_seen_subcommand_from"

# mise
complete -kxc mise -s C -l cd -a "(__fish_complete_directories)" -d 'Change directory before running command'
complete -kxc mise -s q -l quiet -d 'Suppress non-error messages'
complete -kxc mise -s v -l verbose -d 'Show extra output (use -vv for even more)'
complete -kxc mise -s y -l yes -d 'Answer yes to all confirmation prompts'
set -l others activate alias bin-paths cache completion config current deactivate direnv doctor env exec implode install latest link ls ls-remote outdated plugins prune reshim run self-update set settings shell sync task trust uninstall unset upgrade use version watch where which
complete -xc mise -n "not $fssf $others" -a activate -d 'Initializes mise in the current shell session'
complete -xc mise -n "not $fssf $others" -a alias -d 'Manage aliases'
complete -xc mise -n "not $fssf $others" -a bin-paths -d 'List all the active runtime bin paths'
complete -xc mise -n "not $fssf $others" -a cache -d 'Manage the mise cache'
complete -xc mise -n "not $fssf $others" -a completion -d 'Generate shell completions'
complete -xc mise -n "not $fssf $others" -a config -d '[experimental] Manage config files'
complete -xc mise -n "not $fssf $others" -a current -d 'Shows current active and installed runtime versions'
complete -xc mise -n "not $fssf $others" -a deactivate -d 'Disable mise for current shell session'
complete -xc mise -n "not $fssf $others" -a direnv -d 'Output direnv function to use mise inside direnv'
complete -xc mise -n "not $fssf $others" -a doctor -d 'Check mise installation for possible problems.'
complete -xc mise -n "not $fssf $others" -a env -d 'Exports env vars to activate mise a single time'
complete -xc mise -n "not $fssf $others" -a exec -d 'Execute a command with tool(s) set'
complete -xc mise -n "not $fssf $others" -a implode -d 'Removes mise CLI and all related data'
complete -xc mise -n "not $fssf $others" -a install -d 'Install a tool version'
complete -xc mise -n "not $fssf $others" -a latest -d 'Gets the latest available version for a plugin'
complete -xc mise -n "not $fssf $others" -a link -d 'Symlinks a tool version into mise'
complete -xc mise -n "not $fssf $others" -a ls -d 'List installed and/or currently selected tool versions'
complete -xc mise -n "not $fssf $others" -a ls-remote -d 'List runtime versions available for install'
complete -xc mise -n "not $fssf $others" -a outdated -d 'Shows outdated tool versions'
complete -xc mise -n "not $fssf $others" -a plugins -d 'Manage plugins'
complete -xc mise -n "not $fssf $others" -a prune -d 'Delete unused versions of tools'
complete -xc mise -n "not $fssf $others" -a reshim -d 'rebuilds the shim farm'
complete -xc mise -n "not $fssf $others" -a run -d '[experimental] Run a task'
complete -xc mise -n "not $fssf $others" -a self-update -d 'Updates mise itself'
complete -xc mise -n "not $fssf $others" -a set -d 'Manage environment variables'
complete -xc mise -n "not $fssf $others" -a settings -d 'Manage settings'
complete -xc mise -n "not $fssf $others" -a shell -d 'Sets a tool version for the current shell session'
complete -xc mise -n "not $fssf $others" -a sync -d 'Add tool versions from external tools to mise'
complete -xc mise -n "not $fssf $others" -a task -d '[experimental] Manage tasks'
complete -xc mise -n "not $fssf $others" -a trust -d 'Marks a config file as trusted'
complete -xc mise -n "not $fssf $others" -a uninstall -d 'Removes runtime versions'
complete -xc mise -n "not $fssf $others" -a unset -d 'Remove environment variable(s) from the config file'
complete -xc mise -n "not $fssf $others" -a upgrade -d 'Upgrades outdated tool versions'
complete -xc mise -n "not $fssf $others" -a use -d 'Change the active version of a tool locally or globally.'
complete -xc mise -n "not $fssf $others" -a version -d 'Show mise version'
complete -xc mise -n "not $fssf $others" -a watch -d '[experimental] Run a task watching for changes'
complete -xc mise -n "not $fssf $others" -a where -d 'Display the installation path for a runtime'
complete -xc mise -n "not $fssf $others" -a which -d 'Shows the path that a bin name points to'

# activate
complete -kxc mise -n "$fssf activate" -s q -l quiet -d 'Suppress non-error messages'
complete -kxc mise -n "$fssf activate" -a "bash fish nu xonsh zsh" -d 'Shell type to generate the script for'
complete -kxc mise -n "$fssf activate" -l status -d 'Show "mise: <PLUGIN>@<VERSION>" message when changing directories'

# alias
complete -kxc mise -n "$fssf alias" -l no-header -d 'Don'\''t show table header'
complete -kxc mise -n "$fssf alias" -s p -l plugin -a "(__mise_plugins)" -d 'filter aliases by plugin'
set -l others get ls set unset
complete -xc mise -n "$fssf alias; and not $fssf $others" -a get -d 'Show an alias for a plugin'
complete -xc mise -n "$fssf alias; and not $fssf $others" -a ls -d 'List aliases'
complete -xc mise -n "$fssf alias; and not $fssf $others" -a set -d 'Add/update an alias for a plugin'
complete -xc mise -n "$fssf alias; and not $fssf $others" -a unset -d 'Clears an alias for a plugin'

# alias get
complete -kxc mise -n "$fssf alias; and $fssf get" -a "(__mise_aliases)" -d 'The alias to show'
complete -kxc mise -n "$fssf alias; and $fssf get" -a "(__mise_plugins)" -d 'The plugin to show the alias for'

# alias ls
complete -kxc mise -n "$fssf alias; and $fssf ls" -l no-header -d 'Don'\''t show table header'
complete -kxc mise -n "$fssf alias; and $fssf ls" -a "(__mise_plugins)" -d 'Show aliases for <PLUGIN>'

# alias set
complete -kxc mise -n "$fssf alias; and $fssf set" -a "(__mise_aliases)" -d 'The alias to set'
complete -kxc mise -n "$fssf alias; and $fssf set" -a "(__mise_plugins)" -d 'The plugin to set the alias for'
complete -kxc mise -n "$fssf alias; and $fssf set" -d 'The value to set the alias to'

# alias unset
complete -kxc mise -n "$fssf alias; and $fssf unset" -a "(__mise_aliases)" -d 'The alias to remove'
complete -kxc mise -n "$fssf alias; and $fssf unset" -a "(__mise_plugins)" -d 'The plugin to remove the alias from'


# bin-paths

# cache
set -l others clear
complete -xc mise -n "$fssf cache; and not $fssf $others" -a clear -d 'Deletes all cache files in mise'

# cache clear
complete -kxc mise -n "$fssf cache; and $fssf clear" -a "(__mise_plugins)" -d 'Plugin(s) to clear cache for e.g.: node, python'


# completion
complete -kxc mise -n "$fssf completion" -a "bash fish zsh" -d 'Shell type to generate completions for'

# config
complete -kxc mise -n "$fssf config" -l no-header -d 'Do not print table header'
set -l others generate ls
complete -xc mise -n "$fssf config; and not $fssf $others" -a generate -d '[experimental] Generate an .mise.toml file'
complete -xc mise -n "$fssf config; and not $fssf $others" -a ls -d '[experimental] List config files currently in use'

# config generate
complete -kxc mise -n "$fssf config; and $fssf generate" -s o -l output -a "(__fish_complete_path)" -d 'Output to file instead of stdout'

# config ls
complete -kxc mise -n "$fssf config; and $fssf ls" -l no-header -d 'Do not print table header'


# current
complete -kxc mise -n "$fssf current" -a "(__mise_plugins)" -d 'Plugin to show versions of e.g.: ruby, node, cargo:eza, npm:prettier, etc'

# deactivate

# direnv
set -l others activate
complete -xc mise -n "$fssf direnv; and not $fssf $others" -a activate -d 'Output direnv function to use mise inside direnv'

# direnv activate


# doctor

# env
complete -kxc mise -n "$fssf env" -s J -l json -d 'Output in JSON format'
complete -kxc mise -n "$fssf env" -s s -l shell -a "bash fish nu xonsh zsh" -d 'Shell type to generate environment variables for'
complete -kxc mise -n "$fssf env" -a "(__mise_tool_versions)" -d 'Tool(s) to use'

# exec
complete -kxc mise -n "$fssf exec" -s c -l command -d 'Command string to execute'
complete -kxc mise -n "$fssf exec" -d 'Command string to execute (same as --command)'
complete -kxc mise -n "$fssf exec" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf exec" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc mise -n "$fssf exec" -a "(__mise_tool_versions)" -d 'Tool(s) to start e.g.: node@20 python@3.10'

# implode
complete -kxc mise -n "$fssf implode" -l config -d 'Also remove config directory'
complete -kxc mise -n "$fssf implode" -s n -l dry-run -d 'List directories that would be removed without actually removing them'

# install
complete -kxc mise -n "$fssf install" -s f -l force -d 'Force reinstall even if already installed'
complete -kxc mise -n "$fssf install" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf install" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc mise -n "$fssf install" -a "(__mise_tool_versions)" -d 'Tool(s) to install e.g.: node@20'
complete -kxc mise -n "$fssf install" -s v -l verbose -d 'Show installation output'

# latest
complete -kxc mise -n "$fssf latest" -s i -l installed -d 'Show latest installed instead of available version'
complete -kxc mise -n "$fssf latest" -a "(__mise_tool_versions)" -d 'Tool to get the latest version of'

# link
complete -kxc mise -n "$fssf link" -s f -l force -d 'Overwrite an existing tool version if it exists'
complete -kxc mise -n "$fssf link" -a "(__fish_complete_directories)" -d 'The local path to the tool version'
complete -kxc mise -n "$fssf link" -a "(__mise_tool_versions)" -d 'Tool name and version to create a symlink for'

# ls
complete -kxc mise -n "$fssf ls" -s c -l current -d 'Only show tool versions currently specified in a .tool-versions/.mise.toml'
complete -kxc mise -n "$fssf ls" -s g -l global -d 'Only show tool versions currently specified in a the global .tool-versions/.mise.toml'
complete -kxc mise -n "$fssf ls" -s i -l installed -d 'Only show tool versions that are installed Hides missing ones defined in .tool-versions/.mise.toml but not yet installed'
complete -kxc mise -n "$fssf ls" -s J -l json -d 'Output in json format'
complete -kxc mise -n "$fssf ls" -s m -l missing -d 'Display missing tool versions'
complete -kxc mise -n "$fssf ls" -l no-header -d 'Don'\''t display headers'
complete -kxc mise -n "$fssf ls" -a "(__mise_plugins)" -d 'Only show tool versions from [PLUGIN]'
complete -kxc mise -n "$fssf ls" -l prefix -d 'Display versions matching this prefix'

# ls-remote
complete -kxc mise -n "$fssf ls-remote" -l all -d 'Show all installed plugins and versions'
complete -kxc mise -n "$fssf ls-remote" -a "(__mise_plugins)" -d 'Plugin to get versions for'
complete -kxc mise -n "$fssf ls-remote" -d 'The version prefix to use when querying the latest version'

# outdated
complete -kxc mise -n "$fssf outdated" -a "(__mise_tool_versions)" -d 'Tool(s) to show outdated versions for'

# plugins
complete -kxc mise -n "$fssf plugins" -s c -l core -d 'The built-in plugins only'
complete -kxc mise -n "$fssf plugins" -s u -l urls -d 'Show the git url for each plugin'
complete -kxc mise -n "$fssf plugins" -l user -d 'List installed plugins'
set -l others install link ls ls-remote uninstall update
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a install -d 'Install a plugin'
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a link -d 'Symlinks a plugin into mise'
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a ls -d 'List installed plugins'
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a ls-remote -d 'List all available remote plugins'
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a uninstall -d 'Removes a plugin'
complete -xc mise -n "$fssf plugins; and not $fssf $others" -a update -d 'Updates a plugin to the latest version'

# plugins install
complete -kxc mise -n "$fssf plugins; and $fssf install" -s a -l all -d 'Install all missing plugins'
complete -kxc mise -n "$fssf plugins; and $fssf install" -s f -l force -d 'Reinstall even if plugin exists'
complete -kxc mise -n "$fssf plugins; and $fssf install" -d 'The git url of the plugin'
complete -kxc mise -n "$fssf plugins; and $fssf install" -a "(__mise_all_plugins)" -d 'The name of the plugin to install'
complete -kxc mise -n "$fssf plugins; and $fssf install" -s v -l verbose -d 'Show installation output'

# plugins link
complete -kxc mise -n "$fssf plugins; and $fssf link" -s f -l force -d 'Overwrite existing plugin'
complete -kxc mise -n "$fssf plugins; and $fssf link" -d 'The name of the plugin'
complete -kxc mise -n "$fssf plugins; and $fssf link" -a "(__fish_complete_directories)" -d 'The local path to the plugin'

# plugins ls
complete -kxc mise -n "$fssf plugins; and $fssf ls" -s c -l core -d 'The built-in plugins only'
complete -kxc mise -n "$fssf plugins; and $fssf ls" -s u -l urls -d 'Show the git url for each plugin'
complete -kxc mise -n "$fssf plugins; and $fssf ls" -l user -d 'List installed plugins'

# plugins ls-remote
complete -kxc mise -n "$fssf plugins; and $fssf ls-remote" -l only-names -d 'Only show the name of each plugin by default it will show a "*" next to installed plugins'
complete -kxc mise -n "$fssf plugins; and $fssf ls-remote" -s u -l urls -d 'Show the git url for each plugin e.g.: https://github.com/mise-plugins/rtx-nodejs.git'

# plugins uninstall
complete -kxc mise -n "$fssf plugins; and $fssf uninstall" -s a -l all -d 'Remove all plugins'
complete -kxc mise -n "$fssf plugins; and $fssf uninstall" -a "(__mise_plugins)" -d 'Plugin(s) to remove'
complete -kxc mise -n "$fssf plugins; and $fssf uninstall" -s p -l purge -d 'Also remove the plugin'\''s installs, downloads, and cache'

# plugins update
complete -kxc mise -n "$fssf plugins; and $fssf update" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf plugins; and $fssf update" -a "(__mise_plugins)" -d 'Plugin(s) to update'


# prune
complete -kxc mise -n "$fssf prune" -s n -l dry-run -d 'Do not actually delete anything'
complete -kxc mise -n "$fssf prune" -a "(__mise_plugins)" -d 'Prune only versions from this plugin(s)'

# reshim

# run
complete -kxc mise -n "$fssf run" -d 'Arguments to pass to the task. Use ":::" to separate tasks'
complete -kxc mise -n "$fssf run" -s C -l cd -a "(__fish_complete_directories)" -d 'Change to this directory before executing the command'
complete -kxc mise -n "$fssf run" -s n -l dry-run -d 'Don'\''t actually run the task(s), just print them in order of execution'
complete -kxc mise -n "$fssf run" -s f -l force -d 'Force the task to run even if outputs are up to date'
complete -kxc mise -n "$fssf run" -s i -l interleave -d 'Print directly to stdout/stderr instead of by line'
complete -kxc mise -n "$fssf run" -s j -l jobs -d 'Number of tasks to run in parallel'
complete -kxc mise -n "$fssf run" -s p -l prefix -d 'Print stdout/stderr by line, prefixed with the task'\''s label'
complete -kxc mise -n "$fssf run" -s r -l raw -d 'Read/write directly to stdin/stdout/stderr instead of by line'
complete -kxc mise -n "$fssf run" -a "(__mise_tasks)" -d 'Task to run'
complete -kxc mise -n "$fssf run" -s t -l tool -a "(__mise_tool_versions)" -d 'Tool(s) to also add e.g.: node@20 python@3.10'

# self-update
complete -kxc mise -n "$fssf self-update" -s f -l force -d 'Update even if already up to date'
complete -kxc mise -n "$fssf self-update" -l no-plugins -d 'Disable auto-updating plugins'
complete -kxc mise -n "$fssf self-update" -d 'Update to a specific version'
complete -kxc mise -n "$fssf self-update" -s y -l yes -d 'Skip confirmation prompt'

# set
complete -kxc mise -n "$fssf set" -d 'Environment variable(s) to set'
complete -kxc mise -n "$fssf set" -l file -a "(__fish_complete_path)" -d 'The TOML file to update'
complete -kxc mise -n "$fssf set" -s g -l global -d 'Set the environment variable in the global config file'

# settings
set -l others get ls set unset
complete -xc mise -n "$fssf settings; and not $fssf $others" -a get -d 'Show a current setting'
complete -xc mise -n "$fssf settings; and not $fssf $others" -a ls -d 'Show current settings'
complete -xc mise -n "$fssf settings; and not $fssf $others" -a set -d 'Add/update a setting'
complete -xc mise -n "$fssf settings; and not $fssf $others" -a unset -d 'Clears a setting'

# settings get
complete -kxc mise -n "$fssf settings; and $fssf get" -a "(__mise_settings)" -d 'The setting to show'

# settings ls

# settings set
complete -kxc mise -n "$fssf settings; and $fssf set" -a "(__mise_settings)" -d 'The setting to set'
complete -kxc mise -n "$fssf settings; and $fssf set" -d 'The value to set'

# settings unset
complete -kxc mise -n "$fssf settings; and $fssf unset" -a "(__mise_settings)" -d 'The setting to remove'


# shell
complete -kxc mise -n "$fssf shell" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf shell" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc mise -n "$fssf shell" -a "(__mise_tool_versions)" -d 'Tool(s) to use'
complete -kxc mise -n "$fssf shell" -s u -l unset -d 'Removes a previously set version'

# sync
set -l others node python
complete -xc mise -n "$fssf sync; and not $fssf $others" -a node -d 'Symlinks all tool versions from an external tool into mise'
complete -xc mise -n "$fssf sync; and not $fssf $others" -a python -d 'Symlinks all tool versions from an external tool into mise'

# sync node
complete -kxc mise -n "$fssf sync; and $fssf node" -l brew -d 'Get tool versions from Homebrew'
complete -kxc mise -n "$fssf sync; and $fssf node" -l nodenv -d 'Get tool versions from nodenv'
complete -kxc mise -n "$fssf sync; and $fssf node" -l nvm -d 'Get tool versions from nvm'

# sync python
complete -kxc mise -n "$fssf sync; and $fssf python" -l pyenv -d 'Get tool versions from pyenv'


# task
complete -kxc mise -n "$fssf task" -l hidden -d 'Show hidden tasks'
complete -kxc mise -n "$fssf task" -l no-header -d 'Do not print table header'
set -l others deps edit ls run
complete -xc mise -n "$fssf task; and not $fssf $others" -a deps -d '[experimental] Display a tree visualization of a dependency graph'
complete -xc mise -n "$fssf task; and not $fssf $others" -a edit -d '[experimental] Edit a task with $EDITOR'
complete -xc mise -n "$fssf task; and not $fssf $others" -a ls -d '[experimental] List available tasks to execute'
complete -xc mise -n "$fssf task; and not $fssf $others" -a run -d '[experimental] Run a task'

# task deps
complete -kxc mise -n "$fssf task; and $fssf deps" -l dot -d 'Display dependencies in DOT format'
complete -kxc mise -n "$fssf task; and $fssf deps" -d 'Tasks to show dependencies for'

# task edit
complete -kxc mise -n "$fssf task; and $fssf edit" -s p -l path -d 'Display the path to the task instead of editing it'
complete -kxc mise -n "$fssf task; and $fssf edit" -a "(__mise_tasks)" -d 'Task to edit'

# task ls
complete -kxc mise -n "$fssf task; and $fssf ls" -l hidden -d 'Show hidden tasks'
complete -kxc mise -n "$fssf task; and $fssf ls" -l no-header -d 'Do not print table header'

# task run
complete -kxc mise -n "$fssf task; and $fssf run" -d 'Arguments to pass to the task. Use ":::" to separate tasks'
complete -kxc mise -n "$fssf task; and $fssf run" -s C -l cd -a "(__fish_complete_directories)" -d 'Change to this directory before executing the command'
complete -kxc mise -n "$fssf task; and $fssf run" -s n -l dry-run -d 'Don'\''t actually run the task(s), just print them in order of execution'
complete -kxc mise -n "$fssf task; and $fssf run" -s f -l force -d 'Force the task to run even if outputs are up to date'
complete -kxc mise -n "$fssf task; and $fssf run" -s i -l interleave -d 'Print directly to stdout/stderr instead of by line'
complete -kxc mise -n "$fssf task; and $fssf run" -s j -l jobs -d 'Number of tasks to run in parallel'
complete -kxc mise -n "$fssf task; and $fssf run" -s p -l prefix -d 'Print stdout/stderr by line, prefixed with the task'\''s label'
complete -kxc mise -n "$fssf task; and $fssf run" -s r -l raw -d 'Read/write directly to stdin/stdout/stderr instead of by line'
complete -kxc mise -n "$fssf task; and $fssf run" -a "(__mise_tasks)" -d 'Task to run'
complete -kxc mise -n "$fssf task; and $fssf run" -s t -l tool -a "(__mise_tool_versions)" -d 'Tool(s) to also add e.g.: node@20 python@3.10'


# trust
complete -kxc mise -n "$fssf trust" -s a -l all -d 'Trust all config files in the current directory and its parents'
complete -kxc mise -n "$fssf trust" -a "(__fish_complete_path)" -d 'The config file to trust'
complete -kxc mise -n "$fssf trust" -l untrust -d 'No longer trust this config'

# uninstall
complete -kxc mise -n "$fssf uninstall" -s a -l all -d 'Delete all installed versions'
complete -kxc mise -n "$fssf uninstall" -s n -l dry-run -d 'Do not actually delete anything'
complete -kxc mise -n "$fssf uninstall" -a "(__mise_installed_tool_versions)" -d 'Tool(s) to remove'

# unset
complete -kxc mise -n "$fssf unset" -s f -l file -a "(__fish_complete_path)" -d 'Specify a file to use instead of ".mise.toml"'
complete -kxc mise -n "$fssf unset" -s g -l global -d 'Use the global config file'
complete -kxc mise -n "$fssf unset" -d 'Environment variable(s) to remove'

# upgrade
complete -kxc mise -n "$fssf upgrade" -s n -l dry-run -d 'Just print what would be done, don'\''t actually do it'
complete -kxc mise -n "$fssf upgrade" -s i -l interactive -d 'Display multiselect menu to choose which tools to upgrade'
complete -kxc mise -n "$fssf upgrade" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf upgrade" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc mise -n "$fssf upgrade" -a "(__mise_tool_versions)" -d 'Tool(s) to upgrade'

# use
complete -kxc mise -n "$fssf use" -s e -l env -d 'Modify an environment-specific config file like .mise.<env>.toml'
complete -kxc mise -n "$fssf use" -s f -l force -d 'Force reinstall even if already installed'
complete -kxc mise -n "$fssf use" -l fuzzy -d 'Save fuzzy version to config file'
complete -kxc mise -n "$fssf use" -s g -l global -d 'Use the global config file (~/.config/mise/config.toml) instead of the local one'
complete -kxc mise -n "$fssf use" -s j -l jobs -d 'Number of jobs to run in parallel'
complete -kxc mise -n "$fssf use" -s p -l path -a "(__fish_complete_path)" -d 'Specify a path to a config file or directory If a directory is specified, it will look for .mise.toml (default) or .tool-versions'
complete -kxc mise -n "$fssf use" -l pin -d 'Save exact version to config file'
complete -kxc mise -n "$fssf use" -l raw -d 'Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1'
complete -kxc mise -n "$fssf use" -l remove -d 'Remove the plugin(s) from config file'
complete -kxc mise -n "$fssf use" -a "(__mise_tool_versions)" -d 'Tool(s) to add to config file'

# version

# watch
complete -kxc mise -n "$fssf watch" -d 'Extra arguments'
complete -kxc mise -n "$fssf watch" -s g -l glob -d 'Files to watch'
complete -kxc mise -n "$fssf watch" -s t -l task -a "(__mise_tasks)" -d 'Task to run'

# where
complete -kxc mise -n "$fssf where" -a "(__mise_tool_versions)" -d 'Tool(s) to look up'

# which
complete -kxc mise -n "$fssf which" -d 'The bin name to look up'
complete -kxc mise -n "$fssf which" -l plugin -a "(__mise_plugins)" -d 'Show the plugin name instead of the path'
complete -kxc mise -n "$fssf which" -s t -l tool -a "(__mise_tool_versions)" -d 'Use a specific tool@version'
complete -kxc mise -n "$fssf which" -l version -d 'Show the version instead of the path'



function __mise_all_plugins
    if test -z "$__mise_all_plugins_cache"
        set -g __mise_all_plugins_cache (mise plugins ls --all)
    end
    for p in $__mise_all_plugins_cache
        echo $p
    end
end
function __mise_plugins
    if test -z "$__mise_plugins_cache"
        set -g __mise_plugins_cache (mise plugins ls --core --user)
    end
    for p in $__mise_plugins_cache
        echo $p
    end
end
function __mise_tool_versions
    if test -z "$__mise_tool_versions_cache"
        set -g __mise_tool_versions_cache (mise plugins --core --user) (mise ls-remote --all | tac)
    end
    for tv in $__mise_tool_versions_cache
        echo $tv
    end
end
function __mise_installed_tool_versions
    for tv in (mise ls --installed | awk '{print $1 "@" $2}')
        echo $tv
    end
end
function __mise_aliases
    if test -z "$__mise_aliases_cache"
        set -g __mise_aliases_cache (mise alias ls | awk '{print $2}')
    end
    for a in $__mise_aliases_cache
        echo $a
    end
end
function __mise_tasks
    for tv in (mise task ls --no-header | awk '{print $1}')
        echo $tv
    end
end
function __mise_settings
    if test -z "$__mise_settings_cache"
        set -g __mise_settings_cache (mise settings ls | awk '{print $1}')
    end
    for s in $__mise_settings_cache
        echo $s
    end
end

# vim: noet ci pi sts=0 sw=4 ts=4
