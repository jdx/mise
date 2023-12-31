<!-- RTX:COMMANDS -->

## Commands

### `rtx activate [OPTIONS] [SHELL_TYPE]`

```text
Initializes rtx in the current shell session

This should go into your shell's rc file.
Otherwise, it will only take effect in the current session.
(e.g. ~/.zshrc, ~/.bashrc)

This is only intended to be used in interactive sessions, not scripts.
rtx is only capable of updating PATH when the prompt is displayed to the user.
For non-interactive use-cases, use shims instead.

Typically this can be added with something like the following:

    echo 'eval "$(rtx activate)"' >> ~/.zshrc

However, this requires that "rtx" is in your PATH. If it is not, you need to
specify the full path like this:

    echo 'eval "$(/path/to/rtx activate)"' >> ~/.zshrc

Usage: activate [OPTIONS] [SHELL_TYPE]

Arguments:
  [SHELL_TYPE]
          Shell type to generate the script for

          [possible values: bash, fish, nu, xonsh, zsh]

Options:
      --status
          Show "rtx: <PLUGIN>@<VERSION>" message when changing directories

  -q, --quiet
          Suppress non-error messages

Examples:
  $ eval "$(rtx activate bash)"
  $ eval "$(rtx activate zsh)"
  $ rtx activate fish | source
  $ execx($(rtx activate xonsh))
```

### `rtx alias get <PLUGIN> <ALIAS>`

```text
Show an alias for a plugin

This is the contents of an alias.<PLUGIN> entry in ~/.config/rtx/config.toml

Usage: alias get <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to show the alias for

  <ALIAS>
          The alias to show

Examples:
 $ rtx alias get node lts-hydrogen
 20.0.0
```

### `rtx alias ls [OPTIONS] [PLUGIN]`

**Aliases:** `list`

```text
List aliases
Shows the aliases that can be specified.
These can come from user config or from plugins in `bin/list-aliases`.

For user config, aliases are defined like the following in `~/.config/rtx/config.toml`:

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
  $ rtx aliases
  node    lts-hydrogen   20.0.0
```

### `rtx alias set <PLUGIN> <ALIAS> <VALUE>`

**Aliases:** `add, create`

```text
Add/update an alias for a plugin

This modifies the contents of ~/.config/rtx/config.toml

Usage: alias set <PLUGIN> <ALIAS> <VALUE>

Arguments:
  <PLUGIN>
          The plugin to set the alias for

  <ALIAS>
          The alias to set

  <VALUE>
          The value to set the alias to

Examples:
  $ rtx alias set node lts-hydrogen 18.0.0
```

### `rtx alias unset <PLUGIN> <ALIAS>`

**Aliases:** `del, delete, remove, rm`

```text
Clears an alias for a plugin

This modifies the contents of ~/.config/rtx/config.toml

Usage: alias unset <PLUGIN> <ALIAS>

Arguments:
  <PLUGIN>
          The plugin to remove the alias from

  <ALIAS>
          The alias to remove

Examples:
  $ rtx alias unset node lts-hydrogen
```

### `rtx bin-paths`

```text
List all the active runtime bin paths

Usage: bin-paths
```

### `rtx cache clear [PLUGIN]...`

**Aliases:** `c`

```text
Deletes all cache files in rtx

Usage: cache clear [PLUGIN]...

Arguments:
  [PLUGIN]...
          Plugin(s) to clear cache for e.g.: node, python
```

### `rtx completion [SHELL]`

```text
Generate shell completions

Usage: completion [SHELL]

Arguments:
  [SHELL]
          Shell type to generate completions for

          [possible values: bash, fish, zsh]

Examples:
  $ rtx completion bash > /etc/bash_completion.d/rtx
  $ rtx completion zsh  > /usr/local/share/zsh/site-functions/_rtx
  $ rtx completion fish > ~/.config/fish/completions/rtx.fish
```

### `rtx config ls [OPTIONS]`

```text
[experimental] List config files currently in use

Usage: config ls [OPTIONS]

Options:
      --no-header
          Do not print table header

Examples:
  $ rtx config ls
```

### `rtx config generate [OPTIONS]`

**Aliases:** `g`

```text
[experimental] Generate an .rtx.toml file

Usage: config generate [OPTIONS]

Options:
  -o, --output <OUTPUT>
          Output to file instead of stdout

Examples:
  $ rtx cf generate > .rtx.toml
  $ rtx cf generate --output=.rtx.toml
```

### `rtx current [PLUGIN]`

```text
Shows current active and installed runtime versions

This is similar to `rtx ls --current`, but this only shows the runtime
and/or version. It's designed to fit into scripts more easily.

Usage: current [PLUGIN]

Arguments:
  [PLUGIN]
          Plugin to show versions of e.g.: ruby, node

Examples:
  # outputs `.tool-versions` compatible format
  $ rtx current
  python 3.11.0 3.10.0
  shfmt 3.6.0
  shellcheck 0.9.0
  node 20.0.0

  $ rtx current node
  20.0.0

  # can output multiple versions
  $ rtx current python
  3.11.0 3.10.0
```

### `rtx deactivate`

```text
Disable rtx for current shell session

This can be used to temporarily disable rtx in a shell session.

Usage: deactivate

Examples:
  $ rtx deactivate bash
  $ rtx deactivate zsh
  $ rtx deactivate fish
  $ execx($(rtx deactivate xonsh))
```

### `rtx direnv activate`

```text
Output direnv function to use rtx inside direnv

See https://github.com/jdx/rtx#direnv for more information

Because this generates the legacy files based on currently installed plugins,
you should run this command after installing new plugins. Otherwise
direnv may not know to update environment variables when legacy file versions change.

Usage: direnv activate

Examples:
  $ rtx direnv activate > ~/.config/direnv/lib/use_rtx.sh
  $ echo 'use rtx' > .envrc
  $ direnv allow
```

### `rtx doctor`

```text
Check rtx installation for possible problems.

Usage: doctor

Examples:
  $ rtx doctor
  [WARN] plugin node is not installed
```

### `rtx env [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `e`

```text
Exports env vars to activate rtx a single time

Use this if you don't want to permanently install rtx. It's not necessary to
use this if you have `rtx activate` in your shell rc file.

Usage: env [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -s, --shell <SHELL>
          Shell type to generate environment variables for

          [possible values: bash, fish, nu, xonsh, zsh]

  -J, --json
          Output in JSON format

Examples:
  $ eval "$(rtx env -s bash)"
  $ eval "$(rtx env -s zsh)"
  $ rtx env -s fish | source
  $ execx($(rtx env -s xonsh))
```

### `rtx env-vars [OPTIONS] [ENV_VARS]...`

**Aliases:** `ev`

```text
Manage environment variables

By default this command modifies ".rtx.toml" in the current directory.
You can specify the file name by either setting the RTX_DEFAULT_CONFIG_FILENAME environment variable, or by using the --file option.

Usage: env-vars [OPTIONS] [ENV_VARS]...

Arguments:
  [ENV_VARS]...
          Environment variable(s) to set
          e.g.: NODE_ENV=production

Options:
      --file <FILE>
          The TOML file to update

          Defaults to RTX_DEFAULT_CONFIG_FILENAME environment variable, or ".rtx.toml".

      --remove <ENV_VAR>
          Remove the environment variable from config file

          Can be used multiple times.
```

### `rtx exec [OPTIONS] [TOOL@VERSION]... [-- <COMMAND>...]`

**Aliases:** `x`

```text
Execute a command with tool(s) set

use this to avoid modifying the shell session or running ad-hoc commands with rtx tools set.

Tools will be loaded from .rtx.toml/.tool-versions, though they can be overridden with <RUNTIME> args
Note that only the plugin specified will be overridden, so if a `.tool-versions` file
includes "node 20" but you run `rtx exec python@3.11`; it will still load node@20.

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

          [env: RTX_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

Examples:
  $ rtx exec node@20 -- node ./app.js  # launch app.js using node-20.x
  $ rtx x node@20 -- node ./app.js     # shorter alias

  # Specify command as a string:
  $ rtx exec node@20 python@3.11 --command "node -v && python -V"

  # Run a command in a different directory:
  $ rtx x -C /path/to/project node@20 -- node ./app.js
```

### `rtx implode [OPTIONS]`

```text
Removes rtx CLI and all related data

Skips config directory by default.

Usage: implode [OPTIONS]

Options:
      --config
          Also remove config directory

  -n, --dry-run
          List directories that would be removed without actually removing them
```

### `rtx install [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `i`

```text
Install a tool version

This will install a tool version to `~/.local/share/rtx/installs/<PLUGIN>/<VERSION>`
It won't be used simply by being installed, however.
For that, you must set up a `.rtx.toml`/`.tool-version` file manually or with `rtx use`.
Or you can call a tool version explicitly with `rtx exec <TOOL>@<VERSION> -- <COMMAND>`.

Tools will be installed in parallel. To disable, set `--jobs=1` or `RTX_JOBS=1`

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

          [env: RTX_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

  -v, --verbose...
          Show installation output

Examples:
  $ rtx install node@20.0.0  # install specific node version
  $ rtx install node@20      # install fuzzy node version
  $ rtx install node         # install version specified in .tool-versions or .rtx.toml
  $ rtx install                # installs everything specified in .tool-versions or .rtx.toml
```

### `rtx latest [OPTIONS] <TOOL@VERSION>`

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
  $ rtx latest node@20  # get the latest version of node 20
  20.0.0

  $ rtx latest node     # get the latest stable version of node
  20.0.0
```

### `rtx link [OPTIONS] <TOOL@VERSION> <PATH>`

**Aliases:** `ln`

```text
Symlinks a tool version into rtx

Use this for adding installs either custom compiled outside
rtx or built with a different tool.

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
  # build node-20.0.0 with node-build and link it into rtx
  $ node-build 20.0.0 ~/.nodes/20.0.0
  $ rtx link node@20.0.0 ~/.nodes/20.0.0

  # have rtx use the python version provided by Homebrew
  $ brew install node
  $ rtx link node@brew $(brew --prefix node)
  $ rtx use node@brew
```

### `rtx ls [OPTIONS] [PLUGIN]...`

**Aliases:** `list`

```text
List installed and/or currently selected tool versions

Usage: ls [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Only show tool versions from [PLUGIN]

Options:
  -c, --current
          Only show tool versions currently specified in a .tool-versions/.rtx.toml

  -g, --global
          Only show tool versions currently specified in a the global .tool-versions/.rtx.toml

  -i, --installed
          Only show tool versions that are installed Hides missing ones defined in .tool-versions/.rtx.toml but not yet installed

  -J, --json
          Output in json format

  -m, --missing
          Display missing tool versions

      --prefix <PREFIX>
          Display versions matching this prefix

      --no-header
          Don't display headers

Examples:
  $ rtx ls
  node    20.0.0 ~/src/myapp/.tool-versions latest
  python  3.11.0 ~/.tool-versions           3.10
  python  3.10.0

  $ rtx ls --current
  node    20.0.0 ~/src/myapp/.tool-versions 20
  python  3.11.0 ~/.tool-versions           3.11.0

  $ rtx ls --json
  {
    "node": [
      {
        "version": "20.0.0",
        "install_path": "/Users/jdx/.rtx/installs/node/20.0.0",
        "source": {
          "type": ".rtx.toml",
          "path": "/Users/jdx/.rtx.toml"
        }
      }
    ],
    "python": [...]
  }
```

### `rtx ls-remote [OPTIONS] [TOOL@VERSION] [PREFIX]`

```text
List runtime versions available for install

note that the results are cached for 24 hours
run `rtx cache clean` to clear the cache and get fresh results

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
  $ rtx ls-remote node
  18.0.0
  20.0.0

  $ rtx ls-remote node@20
  20.0.0
  20.1.0

  $ rtx ls-remote node 20
  20.0.0
  20.1.0
```

### `rtx outdated [TOOL@VERSION]...`

```text
Shows outdated tool versions

Usage: outdated [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to show outdated versions for
          e.g.: node@20 python@3.10
          If not specified, all tools in global and local configs will be shown

Examples:
  $ rtx outdated
  Plugin  Requested  Current  Latest
  python  3.11       3.11.0   3.11.1
  node    20         20.0.0   20.1.0

  $ rtx outdated node
  Plugin  Requested  Current  Latest
  node    20         20.0.0   20.1.0
```

### `rtx plugins install [OPTIONS] [NEW_PLUGIN] [GIT_URL]`

**Aliases:** `a, add, i`

```text
Install a plugin

note that rtx automatically can install plugins when you install a tool
e.g.: `rtx install node@20` will autoinstall the node plugin

This behavior can be modified in ~/.config/rtx/config.toml

Usage: plugins install [OPTIONS] [NEW_PLUGIN] [GIT_URL]

Arguments:
  [NEW_PLUGIN]
          The name of the plugin to install
          e.g.: node, ruby
          Can specify multiple plugins: `rtx plugins install node ruby python`

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
  $ rtx plugins install node

  # install the node plugin using a specific git url
  $ rtx plugins install node https://github.com/rtx-plugins/rtx-nodejs.git

  # install the node plugin using the git url only
  # (node is inferred from the url)
  $ rtx plugins install https://github.com/rtx-plugins/rtx-nodejs.git

  # install the node plugin using a specific ref
  $ rtx plugins install node https://github.com/rtx-plugins/rtx-nodejs.git#v1.0.0
```

### `rtx plugins link [OPTIONS] <NAME> [PATH]`

**Aliases:** `ln`

```text
Symlinks a plugin into rtx

This is used for developing a plugin.

Usage: plugins link [OPTIONS] <NAME> [PATH]

Arguments:
  <NAME>
          The name of the plugin
          e.g.: node, ruby

  [PATH]
          The local path to the plugin
          e.g.: ./rtx-node

Options:
  -f, --force
          Overwrite existing plugin

Examples:
  # essentially just `ln -s ./rtx-node ~/.local/share/rtx/plugins/node`
  $ rtx plugins link node ./rtx-node

  # infer plugin name as "node"
  $ rtx plugins link ./rtx-node
```

### `rtx plugins ls [OPTIONS]`

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
          e.g.: https://github.com/asdf-vm/asdf-node.git

Examples:
  $ rtx plugins ls
  node
  ruby

  $ rtx plugins ls --urls
  node    https://github.com/asdf-vm/asdf-node.git
  ruby    https://github.com/asdf-vm/asdf-ruby.git
```

### `rtx plugins ls-remote [OPTIONS]`

**Aliases:** `list-all, list-remote`

```text
List all available remote plugins

The full list is here: https://github.com/jdx/rtx/blob/main/src/default_shorthands.rs

Examples:
  $ rtx plugins ls-remote


Usage: plugins ls-remote [OPTIONS]

Options:
  -u, --urls
          Show the git url for each plugin e.g.: https://github.com/rtx-plugins/rtx-nodejs.git

      --only-names
          Only show the name of each plugin by default it will show a "*" next to installed plugins
```

### `rtx plugins uninstall [OPTIONS] [PLUGIN]...`

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
  $ rtx uninstall node
```

### `rtx plugins update [OPTIONS] [PLUGIN]...`

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
  $ rtx plugins update            # update all plugins
  $ rtx plugins update node       # update only node
  $ rtx plugins update node#beta  # specify a ref
```

### `rtx prune [OPTIONS] [PLUGIN]...`

```text
Delete unused versions of tools

rtx tracks which config files have been used in ~/.local/share/rtx/tracked_config_files
Versions which are no longer the latest specified in any of those configs are deleted.
Versions installed only with environment variables (`RTX_<PLUGIN>_VERSION`) will be deleted,
as will versions only referenced on the command line (`rtx exec <PLUGIN>@<VERSION>`).

Usage: prune [OPTIONS] [PLUGIN]...

Arguments:
  [PLUGIN]...
          Prune only versions from these plugins

Options:
  -n, --dry-run
          Do not actually delete anything

Examples:
  $ rtx prune --dry-run
  rm -rf ~/.local/share/rtx/versions/node/20.0.0
  rm -rf ~/.local/share/rtx/versions/node/20.0.1
```

### `rtx reshim`

```text
rebuilds the shim farm

This creates new shims in ~/.local/share/rtx/shims for CLIs that have been added.
rtx will try to do this automatically for commands like `npm i -g` but there are
other ways to install things (like using yarn or pnpm for node) that rtx does
not know about and so it will be necessary to call this explicitly.

If you think rtx should automatically call this for a particular command, please
open an issue on the rtx repo. You can also setup a shell function to reshim
automatically (it's really fast so you don't need to worry about overhead):

npm() {
  command npm "$@"
  rtx reshim
}

Usage: reshim

Examples:
  $ rtx reshim
  $ ~/.local/share/rtx/shims/node -v
  v20.0.0
```

### `rtx run [OPTIONS] [TASK] [ARGS]...`

**Aliases:** `r`

```text
[experimental] Run a task

This command will run a task, or multiple tasks in parallel.
Tasks may have dependencies on other tasks or on source files.
If source is configured on a task, it will only run if the source
files have changed.

Tasks can be defined in .rtx.toml or as standalone scripts.
In .rtx.toml, tasks take this form:

    [tasks.build]
    run = "npm run build"
    sources = ["src/**/*.ts"]
    outputs = ["dist/**/*.js"]

Alternatively, tasks can be defined as standalone scripts.
These must be located in the `.rtx/tasks` directory.
The name of the script will be the name of the task.

    $ cat .rtx/tasks/build<<EOF
    #!/usr/bin/env bash
    npm run build
    EOF
    $ rtx run build

Usage: run [OPTIONS] [TASK] [ARGS]...

Arguments:
  [TASK]
          Task to run
          Can specify multiple tasks by separating with `:::`
          e.g.: rtx run task1 arg1 arg2 ::: task2 arg1 arg2

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
          Configure with `task_output` config or `RTX_TASK_OUTPUT` env var

  -i, --interleave
          Print directly to stdout/stderr instead of by line
          Defaults to true if --jobs == 1
          Configure with `task_output` config or `RTX_TASK_OUTPUT` env var

  -t, --tool <TOOL@VERSION>
          Tool(s) to also add e.g.: node@20 python@3.10

  -j, --jobs <JOBS>
          Number of tasks to run in parallel
          [default: 4]
          Configure with `jobs` config or `RTX_JOBS` env var

          [env: RTX_JOBS=]

  -r, --raw
          Read/write directly to stdin/stdout/stderr instead of by line
          Configure with `raw` config or `RTX_RAW` env var

Examples:
  $ rtx run lint
  Runs the "lint" task. This needs to either be defined in .rtx.toml
  or as a standalone script. See the project README for more information.

  $ rtx run build --force
  Forces the "build" task to run even if its sources are up-to-date.

  $ rtx run test --raw
  Runs "test" with stdin/stdout/stderr all connected to the current terminal.
  This forces `--jobs=1` to prevent interleaving of output.

  $ rtx run lint ::: test ::: check
  Runs the "lint", "test", and "check" tasks in parallel.

  $ rtx task cmd1 arg1 arg2 ::: cmd2 arg1 arg2
  Execute multiple tasks each with their own arguments.
```

### `rtx self-update [OPTIONS] [VERSION]`

```text
Updates rtx itself

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

### `rtx settings get <SETTING>`

```text
Show a current setting

This is the contents of a single entry in ~/.config/rtx/config.toml

Note that aliases are also stored in this file
but managed separately with `rtx aliases get`

Usage: settings get <SETTING>

Arguments:
  <SETTING>
          The setting to show

Examples:
  $ rtx settings get legacy_version_file
  true
```

### `rtx settings ls`

**Aliases:** `list`

```text
Show current settings

This is the contents of ~/.config/rtx/config.toml

Note that aliases are also stored in this file
but managed separately with `rtx aliases`

Usage: settings ls

Examples:
  $ rtx settings
  legacy_version_file = false
```

### `rtx settings set <SETTING> <VALUE>`

**Aliases:** `add, create`

```text
Add/update a setting

This modifies the contents of ~/.config/rtx/config.toml

Usage: settings set <SETTING> <VALUE>

Arguments:
  <SETTING>
          The setting to set

  <VALUE>
          The value to set

Examples:
  $ rtx settings set legacy_version_file true
```

### `rtx settings unset <SETTING>`

**Aliases:** `del, delete, remove, rm`

```text
Clears a setting

This modifies the contents of ~/.config/rtx/config.toml

Usage: settings unset <SETTING>

Arguments:
  <SETTING>
          The setting to remove

Examples:
  $ rtx settings unset legacy_version_file
```

### `rtx shell [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `sh`

```text
Sets a tool version for the current shell session

Only works in a session where rtx is already activated.

Usage: shell [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to use

Options:
  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: RTX_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

  -u, --unset
          Removes a previously set version

Examples:
  $ rtx shell node@20
  $ node -v
  v20.0.0
```

### `rtx sync node <--brew|--nvm|--nodenv>`

```text
Symlinks all tool versions from an external tool into rtx

For example, use this to import all Homebrew node installs into rtx

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
  $ rtx sync node --brew
  $ rtx use -g node@18 - uses Homebrew-provided node
```

### `rtx sync python --pyenv`

```text
Symlinks all tool versions from an external tool into rtx

For example, use this to import all pyenv installs into rtx

Usage: sync python --pyenv

Options:
      --pyenv
          Get tool versions from pyenv

Examples:
  $ pyenv install 3.11.0
  $ rtx sync python --pyenv
  $ rtx use -g python@3.11.0 - uses pyenv-provided python
```

### `rtx task edit [OPTIONS] <TASK>`

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
  $ rtx task edit build
  $ rtx task edit test
```

### `rtx task ls [OPTIONS]`

```text
[experimental] List available tasks to execute
These may be included from the config file or from the project's .rtx/tasks directory
rtx will merge all tasks from all parent directories into this list.

So if you have global tasks in ~/.config/rtx/tasks/* and project-specific tasks in
~/myproject/.rtx/tasks/*, then they'll both be available but the project-specific
tasks will override the global ones if they have the same name.

Usage: task ls [OPTIONS]

Options:
      --no-header
          Do not print table header

      --hidden
          Show hidden tasks

Examples:
  $ rtx task ls
```

### `rtx task run [OPTIONS] [TASK] [ARGS]...`

**Aliases:** `r`

```text
[experimental] Run a task

This command will run a task, or multiple tasks in parallel.
Tasks may have dependencies on other tasks or on source files.
If source is configured on a task, it will only run if the source
files have changed.

Tasks can be defined in .rtx.toml or as standalone scripts.
In .rtx.toml, tasks take this form:

    [tasks.build]
    run = "npm run build"
    sources = ["src/**/*.ts"]
    outputs = ["dist/**/*.js"]

Alternatively, tasks can be defined as standalone scripts.
These must be located in the `.rtx/tasks` directory.
The name of the script will be the name of the task.

    $ cat .rtx/tasks/build<<EOF
    #!/usr/bin/env bash
    npm run build
    EOF
    $ rtx run build

Usage: task run [OPTIONS] [TASK] [ARGS]...

Arguments:
  [TASK]
          Task to run
          Can specify multiple tasks by separating with `:::`
          e.g.: rtx run task1 arg1 arg2 ::: task2 arg1 arg2

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
          Configure with `task_output` config or `RTX_TASK_OUTPUT` env var

  -i, --interleave
          Print directly to stdout/stderr instead of by line
          Defaults to true if --jobs == 1
          Configure with `task_output` config or `RTX_TASK_OUTPUT` env var

  -t, --tool <TOOL@VERSION>
          Tool(s) to also add e.g.: node@20 python@3.10

  -j, --jobs <JOBS>
          Number of tasks to run in parallel
          [default: 4]
          Configure with `jobs` config or `RTX_JOBS` env var

          [env: RTX_JOBS=]

  -r, --raw
          Read/write directly to stdin/stdout/stderr instead of by line
          Configure with `raw` config or `RTX_RAW` env var

Examples:
  $ rtx run lint
  Runs the "lint" task. This needs to either be defined in .rtx.toml
  or as a standalone script. See the project README for more information.

  $ rtx run build --force
  Forces the "build" task to run even if its sources are up-to-date.

  $ rtx run test --raw
  Runs "test" with stdin/stdout/stderr all connected to the current terminal.
  This forces `--jobs=1` to prevent interleaving of output.

  $ rtx run lint ::: test ::: check
  Runs the "lint", "test", and "check" tasks in parallel.

  $ rtx task cmd1 arg1 arg2 ::: cmd2 arg1 arg2
  Execute multiple tasks each with their own arguments.
```

### `rtx trust [OPTIONS] [CONFIG_FILE]`

```text
Marks a config file as trusted

This means rtx will parse the file with potentially dangerous
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
  # trusts ~/some_dir/.rtx.toml
  $ rtx trust ~/some_dir/.rtx.toml

  # trusts .rtx.toml in the current or parent directory
  $ rtx trust
```

### `rtx uninstall [OPTIONS] [TOOL@VERSION]...`

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
  $ rtx uninstall node@18.0.0 # will uninstall specific version
  $ rtx uninstall node        # will uninstall current node version
  $ rtx uninstall --all node@18.0.0 # will uninstall all node versions
```

### `rtx upgrade [OPTIONS] [TOOL@VERSION]...`

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

          [env: RTX_JOBS=]

  -i, --interactive
          Display multiselect menu to choose which tools to upgrade

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1
```

### `rtx use [OPTIONS] [TOOL@VERSION]...`

**Aliases:** `u`

```text
Change the active version of a tool locally or globally.

This will install the tool if it is not already installed.
By default, this will use an `.rtx.toml` file in the current directory.
Use the --global flag to use the global config file instead.
This replaces asdf's `local` and `global` commands, however those are still available in rtx.

Usage: use [OPTIONS] [TOOL@VERSION]...

Arguments:
  [TOOL@VERSION]...
          Tool(s) to add to config file
          e.g.: node@20
          If no version is specified, it will default to @latest

Options:
  -f, --force
          Force reinstall even if already installed

      --fuzzy
          Save fuzzy version to config file
          e.g.: `rtx use --fuzzy node@20` will save 20 as the version
          this is the default behavior unless RTX_ASDF_COMPAT=1

  -g, --global
          Use the global config file (~/.config/rtx/config.toml) instead of the local one

  -e, --env <ENV>
          Modify an environment-specific config file like .rtx.<env>.toml

  -j, --jobs <JOBS>
          Number of jobs to run in parallel
          [default: 4]

          [env: RTX_JOBS=]

      --raw
          Directly pipe stdin/stdout/stderr from plugin to user Sets --jobs=1

      --remove <TOOL>
          Remove the tool(s) from config file

  -p, --path <PATH>
          Specify a path to a config file or directory If a directory is specified, it will look for .rtx.toml (default) or .tool-versions

      --pin
          Save exact version to config file
          e.g.: `rtx use --pin node@20` will save 20.0.0 as the version

          [env: RTX_ASDF_COMPAT=]

Examples:
  # set the current version of node to 20.x in .rtx.toml of current directory
  # will write the fuzzy version (e.g.: 20)
  $ rtx use node@20

  # set the current version of node to 20.x in ~/.config/rtx/config.toml
  # will write the precise version (e.g.: 20.0.0)
  $ rtx use -g --pin node@20

  # sets .rtx.local.toml (which is intended not to be committed to a project)
  $ rtx use --env local node@20

  # sets .rtx.staging.toml (which is used if RTX_ENV=staging)
  $ rtx use --env staging node@20
```

### `rtx version`

```text
Show rtx version

Usage: version
```

### `rtx watch [OPTIONS] [ARGS]...`

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
  $ rtx watch -t build
  Runs the "build" task. Will re-run the task when any of its sources change.
  Uses "sources" from the task definition to determine which files to watch.

  $ rtx watch -t build --glob src/**/*.rs
  Runs the "build" task but specify the files to watch with a glob pattern.
  This overrides the "sources" from the task definition.

  $ rtx run -t build --clear
  Extra arguments are passed to watchexec. See `watchexec --help` for details.
```

### `rtx where <TOOL@VERSION>`

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
  $ rtx where node@20
  /home/jdx/.local/share/rtx/installs/node/20.0.0

  # Show the current, active install directory of node
  # Errors if node is not referenced in any .tool-version file
  $ rtx where node
  /home/jdx/.local/share/rtx/installs/node/20.0.0
```

### `rtx which [OPTIONS] <BIN_NAME>`

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
          e.g.: `rtx which npm --tool=node@20`

Examples:
  $ rtx which node
  /home/username/.local/share/rtx/installs/node/20.0.0/bin/node
  $ rtx which node --plugin
  node
  $ rtx which node --version
  20.0.0
```

<!-- RTX:COMMANDS -->
