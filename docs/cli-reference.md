<!-- MISE:COMMANDS -->

# Commands

## `mise activate [OPTIONS] [SHELL_TYPE]`

```text
Initializes mise in the current shell session

This should go into your shell's rc file.
Otherwise, it will only take effect in the current session.
(e.g. ~/.zshrc, ~/.bashrc)

This is only intended to be used in interactive sessions, not scripts.
mise is only capable of updating PATH when the prompt is displayed to the user.
For non-interactive use-cases, use shims instead.

Typically this can be added with something like the following:

    echo 'eval "$(mise activate)"' >> ~/.zshrc

However, this requires that "mise" is in your PATH. If it is not, you need to
specify the full path like this:

    echo 'eval "$(/path/to/mise activate)"' >> ~/.zshrc

Usage: activate [OPTIONS] [SHELL_TYPE]

Arguments:
  [SHELL_TYPE]
          Shell type to generate the script for

          [possible values: bash, fish, nu, xonsh, zsh]

Options:
      --shims
          Use shims instead of modifying PATH
          Effectively the same as:
              PATH="$HOME/.local/share/mise/shims:$PATH"

  -q, --quiet
          Suppress non-error messages

Examples:
  $ eval "$(mise activate bash)"
  $ eval "$(mise activate zsh)"
  $ mise activate fish | source
  $ execx($(mise activate xonsh))
```

## `mise alias get <PLUGIN> <ALIAS>`

```text
Show an alias for a plugin

This is the contents of an alias.<PLUGIN> entry in ~/.config/mise/config.toml

Usage: alias get <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to show the alias for

  <ALIAS>
          The alias to show

Examples:
 $ mise alias get node lts-hydrogen
 20.0.0
```

## `mise alias ls [OPTIONS] [PLUGIN]`

**Aliases:** `list`

```text
List aliases
Shows the aliases that can be specified.
These can come from user config or from plugins in `bin/list-aliases`.

For user config, aliases are defined like the following in `~/.config/mise/config.toml`:

  [alias.node]
  lts = "20.0.0"

Usage: alias ls [OPTIONS] [PLUGIN]

Arguments:
  [PLUGIN]
          Show aliases for <PLUGIN>

Options:
      --no-header
          Don't show table header

Examples:
  $ mise aliases
  node    lts-hydrogen   20.0.0
```

## `mise alias set <PLUGIN> <ALIAS> <VALUE>`

**Aliases:** `add, create`

```text
Add/update an alias for a plugin

This modifies the contents of ~/.config/mise/config.toml

Usage: alias set <PLUGIN> <ALIAS> <VALUE>

Arguments:
  <PLUGIN>
          The plugin to set the alias for

  <ALIAS>
          The alias to set

  <VALUE>
          The value to set the alias to

Examples:
  $ mise alias set node lts-hydrogen 18.0.0
```

## `mise alias unset <PLUGIN> <ALIAS>`

**Aliases:** `del, delete, remove, rm`

```text
Clears an alias for a plugin

This modifies the contents of ~/.config/mise/config.toml

Usage: alias unset <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to remove the alias from

  <ALIAS>
          The alias to remove

Examples:
  $ mise alias unset node lts-hydrogen
```

## `mise bin-paths`

```text
List all the active runtime bin paths

Usage: bin-paths
```

## `mise cache clear [PLUGIN]...`

**Aliases:** `c`

```text
Deletes all cache files in mise

Usage: cache clear [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to clear cache for e.g.: node, python
```

## `mise completion [SHELL]`

```text
Generate shell completions

Usage: completion [SHELL]

Arguments:
  [SHELL]
          Shell type to generate completions for

          [possible values: bash, fish, zsh]

Examples:
  $ mise completion bash > /etc/bash_completion.d/mise
  $ mise completion zsh  > /usr/local/share/zsh/site-functions/_mise
  $ mise completion fish > ~/.config/fish/completions/mise.fish
```

## `mise config ls [OPTIONS]`

```text
[experimental] List config files currently in use

Usage: config ls [OPTIONS]

Options:
      --no-header
          Do not print table header

Examples:
  $ mise config ls
```

## `mise config generate [OPTIONS]`

**Aliases:** `g`

```text
[experimental] Generate an .mise.toml file

Usage: config generate [OPTIONS]

Options:
  -o, --output <OUTPUT>
          Output to file instead of stdout

Examples:
  $ mise cf generate > .mise.toml
  $ mise cf generate --output=.mise.toml
```

## `mise current [PLUGIN]`

```text
Shows current active and installed runtime versions

This is similar to `mise ls --current`, but this only shows the runtime
and/or version. It's designed to fit into scripts more easily.

Usage: current [PLUGIN]

Arguments:
  [PLUGIN]
          Plugin to show versions of e.g.: ruby, node, cargo:eza, npm:prettier, etc

Examples:
  # outputs `.tool-versions` compatible format
  $ mise current
  python 3.11.0 3.10.0
  shfmt 3.6.0
  shellcheck 0.9.0
  node 20.0.0

  $ mise current node
  20.0.0

  # can output multiple versions
  $ mise current python
  3.11.0 3.10.0
```

## `mise deactivate`

```text
Disable mise for current shell session

This can be used to temporarily disable mise in a shell session.

Usage: deactivate

Examples:
  $ mise deactivate bash
  $ mise deactivate zsh
  $ mise deactivate fish
  $ execx($(mise deactivate xonsh))
```

## `mise direnv activate`

```text
Output direnv function to use mise inside direnv

See https://mise.jdx.dev/direnv.html for more information

Because this generates the legacy files based on currently installed plugins,
you should run this command after installing new plugins. Otherwise
direnv may not know to update environment variables when legacy file versions change.

Usage: direnv activate

Examples:
  $ mise direnv activate > ~/.config/direnv/lib/use_mise.sh
  $ echo 'use mise' > .envrc
  $ direnv allow
```

## `mise doctor`

```text
Check mise installation for possible problems.

Usage: doctor

Examples:
  $ mise doctor
  [WARN] plugin node is not installed
```

## `mise env [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `e`

```text
Exports env vars to activate mise a single time

Use this if you don't want to permanently install mise. It's not necessary to
use this if you have `mise activate` in your shell rc file.

Usage: env [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -J, --json
          Output in JSON format

  -s, --shell <SHELL>
          Shell type to generate environment variables for

          [possible values: bash, fish, nu, xonsh, zsh]

Examples:
  $ eval "$(mise env -s bash)"
  $ eval "$(mise env -s zsh)"
  $ mise env -s fish | source
  $ execx($(mise env -s xonsh))
```

## `mise exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]`

**Aliases:** `x`

```text
Execute a command with tool(s) set

use this to avoid modifying the shell session or running ad-hoc commands with mise tools set.

Tools will be loaded from .mise.toml/.tool-versions, though they can be overridden with <RUNTIME> args
Note that only the plugin specified will be overridden, so if a `.tool-versions` file
includes "node 20" but you run `mise exec python@3.11`; it will still load node@20.

The "--" separates runtimes from the commands to pass along to the subprocess.

Usage: exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]

Arguments:
  [TOOL@VERSION]...
          Tool(s) to start e.g.: node@20 python@3.10

  [COMMAND]...
          Command string to execute (same as --command)

Options:
  -c, --command <C>
          Command string to execute

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: MISE_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

Examples:
  $ mise exec node@20 -- node ./app.js  # launch app.js using node-20.x
  $ mise x node@20 -- node ./app.js     # shorter alias

  # Specify command as a string:
  $ mise exec node@20 python@3.11 --command "node -v && python -V"

  # Run a command in a different directory:
  $ mise x -C /path/to/project node@20 -- node ./app.js
```

## `mise implode [OPTIONS]`

```text
Removes mise CLI and all related data

Skips config directory by default.

Usage: implode [OPTIONS]

Options:
      --config
          Also remove config directory

  -n, --dry-run
          List directories that would be removed without actually removing them
```

## `mise install [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `i`

```text
Install a tool version

This will install a tool version to `~/.local/share/mise/installs/<PLUGIN>/<VERSION>`
It won't be used simply by being installed, however.
For that, you must set up a `.mise.toml`/`.tool-version` file manually or with `mise use`.
Or you can call a tool version explicitly with `mise exec <TOOL>@<VERSION> -- <COMMAND>`.

Tools will be installed in parallel. To disable, set `--jobs=1` or `MISE_JOBS=1`

Usage: install [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to install e.g.: node@20

Options:
  -f, --force
          Force reinstall even if already installed

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: MISE_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

  -v, --verbose...
          Show installation output

Examples:
  $ mise install node@20.0.0  # install specific node version
  $ mise install node@20      # install fuzzy node version
  $ mise install node         # install version specified in .tool-versions or .mise.toml
  $ mise install              # installs everything specified in .tool-versions or .mise.toml
```

## `mise latest [OPTIONS] <TOOL@VERSION>`

```text
Gets the latest available version for a plugin

Usage: latest [OPTIONS] <TOOL@VERSION>

Arguments:
  <TOOL@VERSION>
          Tool to get the latest version of

Options:
  -i, --installed
          Show latest installed instead of available version

Examples:
  $ mise latest node@20  # get the latest version of node 20
  20.0.0

  $ mise latest node     # get the latest stable version of node
  20.0.0
```

## `mise link [OPTIONS] <TOOL@VERSION> <PATH>`

**Aliases:** `ln`

```text
Symlinks a tool version into mise

Use this for adding installs either custom compiled outside
mise or built with a different tool.

Usage: link [OPTIONS] <TOOL@VERSION> <PATH>

Arguments:
  <TOOL@VERSION>
          Tool name and version to create a symlink for

  <PATH>
          The local path to the tool version
          e.g.: ~/.nvm/versions/node/v20.0.0

Options:
  -f, --force
          Overwrite an existing tool version if it exists

Examples:
  # build node-20.0.0 with node-build and link it into mise
  $ node-build 20.0.0 ~/.nodes/20.0.0
  $ mise link node@20.0.0 ~/.nodes/20.0.0

  # have mise use the python version provided by Homebrew
  $ brew install node
  $ mise link node@brew $(brew --prefix node)
  $ mise use node@brew
```

## `mise ls [OPTIONS] [PLUGIN]...`

**Aliases:** `list`

```text
List installed and/or currently selected tool versions

Usage: ls [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Only show tool versions from [PLUGIN]

Options:
  -c, --current
          Only show tool versions currently specified in a .tool-versions/.mise.toml

  -g, --global
          Only show tool versions currently specified in a the global .tool-versions/.mise.toml

  -i, --installed
          Only show tool versions that are installed Hides missing ones defined in .tool-versions/.mise.toml but not yet installed

  -J, --json
          Output in json format

  -m, --missing
          Display missing tool versions

      --prefix <PREFIX>
          Display versions matching this prefix

      --no-header
          Don't display headers

Examples:
  $ mise ls
  node    20.0.0 ~/src/myapp/.tool-versions latest
  python  3.11.0 ~/.tool-versions           3.10
  python  3.10.0

  $ mise ls --current
  node    20.0.0 ~/src/myapp/.tool-versions 20
  python  3.11.0 ~/.tool-versions           3.11.0

  $ mise ls --json
  {
    "node": [
      {
        "version": "20.0.0",
        "install_path": "/Users/jdx/.mise/installs/node/20.0.0",
        "source": {
          "type": ".mise.toml",
          "path": "/Users/jdx/.mise.toml"
        }
      }
    ],
    "python": [...]
  }
```

## `mise ls-remote [OPTIONS] [TOOL@VERSION] [PREFIX]`

```text
List runtime versions available for install

note that the results are cached for 24 hours
run `mise cache clean` to clear the cache and get fresh results

Usage: ls-remote [OPTIONS] [TOOL@VERSION] [PREFIX]

Arguments:
  [TOOL@VERSION]
          Plugin to get versions for

  [PREFIX]
          The version prefix to use when querying the latest version
          same as the first argument after the "@"

Options:
      --all
          Show all installed plugins and versions

Examples:
  $ mise ls-remote node
  18.0.0
  20.0.0

  $ mise ls-remote node@20
  20.0.0
  20.1.0

  $ mise ls-remote node 20
  20.0.0
  20.1.0
```

## `mise outdated [TOOL@VERSION]...`

```text
Shows outdated tool versions

Usage: outdated [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to show outdated versions for
          e.g.: node@20 python@3.10
          If not specified, all tools in global and local configs will be shown

Examples:
  $ mise outdated
  Plugin  Requested  Current  Latest
  python  3.11       3.11.0   3.11.1
  node    20         20.0.0   20.1.0

  $ mise outdated node
  Plugin  Requested  Current  Latest
  node    20         20.0.0   20.1.0
```

## `mise plugins install [OPTIONS] [NEW_PLUGIN] [GIT_URL]`

**Aliases:** `a, add, i`

```text
Install a plugin

note that mise automatically can install plugins when you install a tool
e.g.: `mise install node@20` will autoinstall the node plugin

This behavior can be modified in ~/.config/mise/config.toml

Usage: plugins install [OPTIONS] [NEW_PLUGIN] [GIT_URL]

Arguments:
  [NEW_PLUGIN]
          The name of the plugin to install
          e.g.: node, ruby
          Can specify multiple plugins: `mise plugins install node ruby python`

  [GIT_URL]
          The git url of the plugin

Options:
  -f, --force
          Reinstall even if plugin exists

  -a, --all
          Install all missing plugins
          This will only install plugins that have matching shorthands.
          i.e.: they don't need the full git repo url

  -v, --verbose...
          Show installation output

Examples:
  # install the node via shorthand
  $ mise plugins install node

  # install the node plugin using a specific git url
  $ mise plugins install node https://github.com/mise-plugins/rtx-nodejs.git

  # install the node plugin using the git url only
  # (node is inferred from the url)
  $ mise plugins install https://github.com/mise-plugins/rtx-nodejs.git

  # install the node plugin using a specific ref
  $ mise plugins install node https://github.com/mise-plugins/rtx-nodejs.git#v1.0.0
```

## `mise plugins link [OPTIONS] <NAME> [PATH]`

**Aliases:** `ln`

```text
Symlinks a plugin into mise

This is used for developing a plugin.

Usage: plugins link [OPTIONS] <NAME> [PATH]

Arguments:
  <NAME>
          The name of the plugin
          e.g.: node, ruby

  [PATH]
          The local path to the plugin
          e.g.: ./mise-node

Options:
  -f, --force
          Overwrite existing plugin

Examples:
  # essentially just `ln -s ./mise-node ~/.local/share/mise/plugins/node`
  $ mise plugins link node ./mise-node

  # infer plugin name as "node"
  $ mise plugins link ./mise-node
```

## `mise plugins ls [OPTIONS]`

**Aliases:** `list`

```text
List installed plugins

Can also show remotely available plugins to install.

Usage: plugins ls [OPTIONS]

Options:
  -c, --core
          The built-in plugins only
          Normally these are not shown

      --user
          List installed plugins

          This is the default behavior but can be used with --core
          to show core and user plugins

  -u, --urls
          Show the git url for each plugin
          e.g.: https://github.com/asdf-vm/asdf-nodejs.git

Examples:
  $ mise plugins ls
  node
  ruby

  $ mise plugins ls --urls
  node    https://github.com/asdf-vm/asdf-nodejs.git
  ruby    https://github.com/asdf-vm/asdf-ruby.git
```

## `mise plugins ls-remote [OPTIONS]`

**Aliases:** `list-all, list-remote`

```text
List all available remote plugins

The full list is here: https://github.com/jdx/mise/blob/main/src/default_shorthands.rs

Examples:
  $ mise plugins ls-remote


Usage: plugins ls-remote [OPTIONS]

Options:
  -u, --urls
          Show the git url for each plugin e.g.: https://github.com/mise-plugins/rtx-nodejs.git

      --only-names
          Only show the name of each plugin by default it will show a "*" next to installed plugins
```

## `mise plugins uninstall [OPTIONS] [PLUGIN]...`

**Aliases:** `remove, rm`

```text
Removes a plugin

Usage: plugins uninstall [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to remove

Options:
  -p, --purge
          Also remove the plugin's installs, downloads, and cache

  -a, --all
          Remove all plugins

Examples:
  $ mise uninstall node
```

## `mise plugins update [OPTIONS] [PLUGIN]...`

**Aliases:** `upgrade`

```text
Updates a plugin to the latest version

note: this updates the plugin itself, not the runtime versions

Usage: plugins update [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to update

Options:
  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          Default: 4

Examples:
  $ mise plugins update            # update all plugins
  $ mise plugins update node       # update only node
  $ mise plugins update node#beta  # specify a ref
```

## `mise prune [OPTIONS] [PLUGIN]...`

```text
Delete unused versions of tools

mise tracks which config files have been used in ~/.local/share/mise/tracked_config_files
Versions which are no longer the latest specified in any of those configs are deleted.
Versions installed only with environment variables (`MISE_<PLUGIN>_VERSION`) will be deleted,
as will versions only referenced on the command line (`mise exec <PLUGIN>@<VERSION>`).

Usage: prune [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Prune only versions from this plugin(s)

Options:
  -n, --dry-run
          Do not actually delete anything

Examples:
  $ mise prune --dry-run
  rm -rf ~/.local/share/mise/versions/node/20.0.0
  rm -rf ~/.local/share/mise/versions/node/20.0.1
```

## `mise reshim`

```text
rebuilds the shim farm

This creates new shims in ~/.local/share/mise/shims for CLIs that have been added.
mise will try to do this automatically for commands like `npm i -g` but there are
other ways to install things (like using yarn or pnpm for node) that mise does
not know about and so it will be necessary to call this explicitly.

If you think mise should automatically call this for a particular command, please
open an issue on the mise repo. You can also setup a shell function to reshim
automatically (it's really fast so you don't need to worry about overhead):

npm() {
  command npm "$@"
  mise reshim
}

Usage: reshim

Examples:
  $ mise reshim
  $ ~/.local/share/mise/shims/node -v
  v20.0.0
```

## `mise run [OPTIONS] [TASK] [ARGS]...`

**Aliases:** `r`

```text
[experimental] Run a task

This command will run a task, or multiple tasks in parallel.
Tasks may have dependencies on other tasks or on source files.
If source is configured on a task, it will only run if the source
files have changed.

Tasks can be defined in .mise.toml or as standalone scripts.
In .mise.toml, tasks take this form:

    [tasks.build]
    run = "npm run build"
    sources = ["src/**/*.ts"]
    outputs = ["dist/**/*.js"]

Alternatively, tasks can be defined as standalone scripts.
These must be located in the `.mise/tasks` directory.
The name of the script will be the name of the task.

    $ cat .mise/tasks/build<<EOF
    #!/usr/bin/env bash
    npm run build
    EOF
    $ mise run build

Usage: run [OPTIONS] [TASK] [ARGS]...

Arguments:
  [TASK]
          Task to run
          Can specify multiple tasks by separating with `:::`
          e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2

          [default: default]

  [ARGS]...
          Arguments to pass to the task. Use ":::" to separate tasks

Options:
  -C, --cd <CD>
          Change to this directory before executing the command

  -n, --dry-run
          Don't actually run the task(s), just print them in order of execution

  -f, --force
          Force the task to run even if outputs are up to date

  -p, --prefix
          Print stdout/stderr by line, prefixed with the task's label
          Defaults to true if --jobs > 1
          Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

  -i, --interleave
          Print directly to stdout/stderr instead of by line
          Defaults to true if --jobs == 1
          Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

  -t, --tool <TOOL@VERSION>
          Tool(s) to also add e.g.: node@20 python@3.10

  -j, --jobs <JOBS>
          Number of tasks to run in parallel
          [default: 4]
          Configure with `jobs` config or `MISE_JOBS` env var

          [env: MISE_JOBS=]

  -r, --raw
          Read/write directly to stdin/stdout/stderr instead of by line
          Configure with `raw` config or `MISE_RAW` env var

      --timings
          Shows elapsed time after each task

Examples:
  $ mise run lint
  Runs the "lint" task. This needs to either be defined in .mise.toml
  or as a standalone script. See the project README for more information.

  $ mise run build --force
  Forces the "build" task to run even if its sources are up-to-date.

  $ mise run test --raw
  Runs "test" with stdin/stdout/stderr all connected to the current terminal.
  This forces `--jobs=1` to prevent interleaving of output.

  $ mise run lint ::: test ::: check
  Runs the "lint", "test", and "check" tasks in parallel.

  $ mise task cmd1 arg1 arg2 ::: cmd2 arg1 arg2
  Execute multiple tasks each with their own arguments.
```

## `mise self-update [OPTIONS] [VERSION]`

```text
Updates mise itself

Uses the GitHub Releases API to find the latest release and binary
By default, this will also update any installed plugins

Usage: self-update [OPTIONS] [VERSION]

Arguments:
  [VERSION]
          Update to a specific version

Options:
  -f, --force
          Update even if already up to date

      --no-plugins
          Disable auto-updating plugins

  -y, --yes
          Skip confirmation prompt
```

## `mise set [OPTIONS] [ENV_VARS]...`

```text
Manage environment variables

By default this command modifies ".mise.toml" in the current directory.

Usage: set [OPTIONS] [ENV_VARS]...

Arguments:
  [ENV_VARS]...
          Environment variable(s) to set
          e.g.: NODE_ENV=production

Options:
      --file <FILE>
          The TOML file to update

          Defaults to MISE_DEFAULT_CONFIG_FILENAME environment variable, or ".mise.toml".

  -g, --global
          Set the environment variable in the global config file

Examples:
  $ mise set NODE_ENV=production

  $ mise set NODE_ENV
  production

  $ mise set
  key       value       source
  NODE_ENV  production  ~/.config/mise/config.toml
```

## `mise settings get <SETTING>`

```text
Show a current setting

This is the contents of a single entry in ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases get`

Usage: settings get <SETTING>

Arguments:
  <SETTING>
          The setting to show

Examples:
  $ mise settings get legacy_version_file
  true
```

## `mise settings ls`

**Aliases:** `list`

```text
Show current settings

This is the contents of ~/.config/mise/config.toml

Note that aliases are also stored in this file
but managed separately with `mise aliases`

Usage: settings ls

Examples:
  $ mise settings
  legacy_version_file = false
```

## `mise settings set <SETTING> <VALUE>`

**Aliases:** `add, create`

```text
Add/update a setting

This modifies the contents of ~/.config/mise/config.toml

Usage: settings set <SETTING> <VALUE>

Arguments:
  <SETTING>
          The setting to set

  <VALUE>
          The value to set

Examples:
  $ mise settings set legacy_version_file true
```

## `mise settings unset <SETTING>`

**Aliases:** `del, delete, remove, rm`

```text
Clears a setting

This modifies the contents of ~/.config/mise/config.toml

Usage: settings unset <SETTING>

Arguments:
  <SETTING>
          The setting to remove

Examples:
  $ mise settings unset legacy_version_file
```

## `mise shell [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `sh`

```text
Sets a tool version for the current shell session

Only works in a session where mise is already activated.

Usage: shell [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: MISE_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

  -u, --unset
          Removes a previously set version

Examples:
  $ mise shell node@20
  $ node -v
  v20.0.0
```

## `mise sync node <--brew|--nvm|--nodenv>`

```text
Symlinks all tool versions from an external tool into mise

For example, use this to import all Homebrew node installs into mise

Usage: sync node <--brew|--nvm|--nodenv>

Options:
      --brew
          Get tool versions from Homebrew

      --nvm
          Get tool versions from nvm

      --nodenv
          Get tool versions from nodenv

Examples:
  $ brew install node@18 node@20
  $ mise sync node --brew
  $ mise use -g node@18 - uses Homebrew-provided node
```

## `mise sync python --pyenv`

```text
Symlinks all tool versions from an external tool into mise

For example, use this to import all pyenv installs into mise

Usage: sync python --pyenv

Options:
      --pyenv
          Get tool versions from pyenv

Examples:
  $ pyenv install 3.11.0
  $ mise sync python --pyenv
  $ mise use -g python@3.11.0 - uses pyenv-provided python
```

## `mise task deps [OPTIONS] [TASKS]...`

```text
[experimental] Display a tree visualization of a dependency graph

Usage: task deps [OPTIONS] [TASKS]...

Arguments:
  [TASKS]...
          Tasks to show dependencies for
          Can specify multiple tasks by separating with spaces
          e.g.: mise task deps lint test check

Options:
      --dot
          Display dependencies in DOT format

Examples:
  $ mise task deps
  Shows dependencies for all tasks

  $ mise task deps lint test check
  Shows dependencies for the "lint", "test" and "check" tasks

  $ mise task deps --dot
  Shows dependencies in DOT format
```

## `mise task edit [OPTIONS] <TASK>`

```text
[experimental] Edit a task with $EDITOR

The task will be created as a standalone script if it does not already exist.

Usage: task edit [OPTIONS] <TASK>

Arguments:
  <TASK>
          Task to edit

Options:
  -p, --path
          Display the path to the task instead of editing it

Examples:
  $ mise task edit build
  $ mise task edit test
```

## `mise task ls [OPTIONS]`

```text
[experimental] List available tasks to execute
These may be included from the config file or from the project's .mise/tasks directory
mise will merge all tasks from all parent directories into this list.

So if you have global tasks in ~/.config/mise/tasks/* and project-specific tasks in
~/myproject/.mise/tasks/*, then they'll both be available but the project-specific
tasks will override the global ones if they have the same name.

Usage: task ls [OPTIONS]

Options:
      --no-header
          Do not print table header

      --hidden
          Show hidden tasks

Examples:
  $ mise task ls
```

## `mise task run [OPTIONS] [TASK] [ARGS]...`

**Aliases:** `r`

```text
[experimental] Run a task

This command will run a task, or multiple tasks in parallel.
Tasks may have dependencies on other tasks or on source files.
If source is configured on a task, it will only run if the source
files have changed.

Tasks can be defined in .mise.toml or as standalone scripts.
In .mise.toml, tasks take this form:

    [tasks.build]
    run = "npm run build"
    sources = ["src/**/*.ts"]
    outputs = ["dist/**/*.js"]

Alternatively, tasks can be defined as standalone scripts.
These must be located in the `.mise/tasks` directory.
The name of the script will be the name of the task.

    $ cat .mise/tasks/build<<EOF
    #!/usr/bin/env bash
    npm run build
    EOF
    $ mise run build

Usage: task run [OPTIONS] [TASK] [ARGS]...

Arguments:
  [TASK]
          Task to run
          Can specify multiple tasks by separating with `:::`
          e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2

          [default: default]

  [ARGS]...
          Arguments to pass to the task. Use ":::" to separate tasks

Options:
  -C, --cd <CD>
          Change to this directory before executing the command

  -n, --dry-run
          Don't actually run the task(s), just print them in order of execution

  -f, --force
          Force the task to run even if outputs are up to date

  -p, --prefix
          Print stdout/stderr by line, prefixed with the task's label
          Defaults to true if --jobs > 1
          Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

  -i, --interleave
          Print directly to stdout/stderr instead of by line
          Defaults to true if --jobs == 1
          Configure with `task_output` config or `MISE_TASK_OUTPUT` env var

  -t, --tool <TOOL@VERSION>
          Tool(s) to also add e.g.: node@20 python@3.10

  -j, --jobs <JOBS>
          Number of tasks to run in parallel
          [default: 4]
          Configure with `jobs` config or `MISE_JOBS` env var

          [env: MISE_JOBS=]

  -r, --raw
          Read/write directly to stdin/stdout/stderr instead of by line
          Configure with `raw` config or `MISE_RAW` env var

      --timings
          Shows elapsed time after each task

Examples:
  $ mise run lint
  Runs the "lint" task. This needs to either be defined in .mise.toml
  or as a standalone script. See the project README for more information.

  $ mise run build --force
  Forces the "build" task to run even if its sources are up-to-date.

  $ mise run test --raw
  Runs "test" with stdin/stdout/stderr all connected to the current terminal.
  This forces `--jobs=1` to prevent interleaving of output.

  $ mise run lint ::: test ::: check
  Runs the "lint", "test", and "check" tasks in parallel.

  $ mise task cmd1 arg1 arg2 ::: cmd2 arg1 arg2
  Execute multiple tasks each with their own arguments.
```

## `mise trust [OPTIONS] [CONFIG_FILE]`

```text
Marks a config file as trusted

This means mise will parse the file with potentially dangerous
features enabled.

This includes:
- environment variables
- templates
- `path:` plugin versions

Usage: trust [OPTIONS] [CONFIG_FILE]

Arguments:
  [CONFIG_FILE]
          The config file to trust

Options:
  -a, --all
          Trust all config files in the current directory and its parents

      --untrust
          No longer trust this config

Examples:
  # trusts ~/some_dir/.mise.toml
  $ mise trust ~/some_dir/.mise.toml

  # trusts .mise.toml in the current or parent directory
  $ mise trust
```

## `mise uninstall [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `remove, rm`

```text
Removes runtime versions

Usage: uninstall [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to remove

Options:
  -a, --all
          Delete all installed versions

  -n, --dry-run
          Do not actually delete anything

Examples:
  $ mise uninstall node@18.0.0 # will uninstall specific version
  $ mise uninstall node        # will uninstall current node version
  $ mise uninstall --all node@18.0.0 # will uninstall all node versions
```

## `mise unset [OPTIONS] [KEYS]...`

```text
Remove environment variable(s) from the config file

By default this command modifies ".mise.toml" in the current directory.

Usage: unset [OPTIONS] [KEYS]...

Arguments:
  [KEYS]...
          Environment variable(s) to remove
          e.g.: NODE_ENV

Options:
  -f, --file <FILE>
          Specify a file to use instead of ".mise.toml"

  -g, --global
          Use the global config file
```

## `mise upgrade [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `up`

```text
Upgrades outdated tool versions

Usage: upgrade [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to upgrade
          e.g.: node@20 python@3.10
          If not specified, all current tools will be upgraded

Options:
  -n, --dry-run
          Just print what would be done, don't actually do it

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: MISE_JOBS=]

  -i, --interactive
          Display multiselect menu to choose which tools to upgrade

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1
```

## `mise use [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `u`

```text
Change the active version of a tool locally or globally.

This will install the tool if it is not already installed.
By default, this will use an `.mise.toml` file in the current directory.
Use the --global flag to use the global config file instead.
This replaces asdf's `local` and `global` commands, however those are still available in mise.

Usage: use [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to add to config file
          e.g.: node@20, cargo:ripgrep@latest npm:prettier@3
          If no version is specified, it will default to @latest

Options:
  -f, --force
          Force reinstall even if already installed

      --fuzzy
          Save fuzzy version to config file
          e.g.: `mise use --fuzzy node@20` will save 20 as the version
          this is the default behavior unless MISE_ASDF_COMPAT=1

  -g, --global
          Use the global config file (~/.config/mise/config.toml) instead of the local one

  -e, --env <ENV>
          Modify an environment-specific config file like .mise.<env>.toml

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: MISE_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

      --remove <PLUGIN>
          Remove the plugin(s) from config file

  -p, --path <PATH>
          Specify a path to a config file or directory If a directory is specified, it will look for .mise.toml (default) or .tool-versions

      --pin
          Save exact version to config file
          e.g.: `mise use --pin node@20` will save 20.0.0 as the version
          Set MISE_ASDF_COMPAT=1 to make this the default behavior

Examples:
  # set the current version of node to 20.x in .mise.toml of current directory
  # will write the fuzzy version (e.g.: 20)
  $ mise use node@20

  # set the current version of node to 20.x in ~/.config/mise/config.toml
  # will write the precise version (e.g.: 20.0.0)
  $ mise use -g --pin node@20

  # sets .mise.local.toml (which is intended not to be committed to a project)
  $ mise use --env local node@20

  # sets .mise.staging.toml (which is used if MISE_ENV=staging)
  $ mise use --env staging node@20
```

## `mise version`

```text
Show mise version

Usage: version
```

## `mise watch [OPTIONS] [ARGS]...`

**Aliases:** `w`

```text
[experimental] Run a task watching for changes

Usage: watch [OPTIONS] [ARGS]...

Arguments:
  [ARGS]...
          Extra arguments

Options:
  -t, --task <TASK>
          Task to run

          [default: default]

  -g, --glob <GLOB>
          Files to watch
          Defaults to sources from the task(s)

Examples:
  $ mise watch -t build
  Runs the "build" task. Will re-run the task when any of its sources change.
  Uses "sources" from the task definition to determine which files to watch.

  $ mise watch -t build --glob src/**/*.rs
  Runs the "build" task but specify the files to watch with a glob pattern.
  This overrides the "sources" from the task definition.

  $ mise run -t build --clear
  Extra arguments are passed to watchexec. See `watchexec --help` for details.
```

## `mise where <TOOL@VERSION>`

```text
Display the installation path for a runtime

Must be installed.

Usage: where <TOOL@VERSION>

Arguments:
  <TOOL@VERSION>
          Tool(s) to look up
          e.g.: ruby@3
          if "@<PREFIX>" is specified, it will show the latest installed version
          that matches the prefix
          otherwise, it will show the current, active installed version

Examples:
  # Show the latest installed version of node
  # If it is is not installed, errors
  $ mise where node@20
  /home/jdx/.local/share/mise/installs/node/20.0.0

  # Show the current, active install directory of node
  # Errors if node is not referenced in any .tool-version file
  $ mise where node
  /home/jdx/.local/share/mise/installs/node/20.0.0
```

## `mise which [OPTIONS] <BIN_NAME>`

```text
Shows the path that a bin name points to

Usage: which [OPTIONS] <BIN_NAME>

Arguments:
  <BIN_NAME>
          The bin name to look up

Options:
      --plugin
          Show the plugin name instead of the path

      --version
          Show the version instead of the path

  -t, --tool <TOOL@VERSION>
          Use a specific tool@version
          e.g.: `mise which npm --tool=node@20`

Examples:
  $ mise which node
  /home/username/.local/share/mise/installs/node/20.0.0/bin/node
  $ mise which node --plugin
  node
  $ mise which node --version
  20.0.0
```

<!-- MISE:COMMANDS -->
