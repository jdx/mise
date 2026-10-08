---
description: "Choose between shell activation, shims and mise exec, and run the right tool version from editors and scripts."
---

# Shims

A shim is a small executable named after a tool's command, such as `node`.
When a program runs it, the shim asks mise which version the current directory
selects, loads that project's environment, and runs the real executable. Put
the shim directory on `PATH` for editors and other programs that never see an
activated shell.

## Choose how to use mise tools {#overview}

| Method                                                    | Environment applies to                              | Updates                                     | Use it for                              |
| --------------------------------------------------------- | --------------------------------------------------- | ------------------------------------------- | --------------------------------------- |
| [`mise activate`](/cli/activate.html)                     | Your current shell                                  | At each prompt and, in most shells, on `cd` | Interactive terminals                   |
| [`mise activate --shims`](#how-to-add-mise-shims-to-path) | Adds the shim directories to the shell's `PATH`     | Each time a shimmed command runs            | Editors, IDEs, GUI apps, login profiles |
| A shim                                                    | The program it launches and that program's children | Each time it runs                           | Commands found through `PATH`           |
| [`mise exec`](/cli/exec.html)                             | One command and its children                        | Once per command                            | Scripts and CI                          |
| [`mise run`](/cli/run.html)                               | A task and its dependencies                         | Once per task                               | Named project commands                  |
| [`mise env`](/cli/env.html)                               | Prints assignments for another program to apply     | When you run it                             | Other environment tools                 |

Use `mise activate` in the shell you type into, so that `node` and `[env]`
variables follow you between projects. Use shims where a program needs a
stable path to a tool, such as an IDE configured with a Python executable;
[Shims vs PATH activation](#shims-vs-path) lists what they leave out. You can
do both: put `mise activate --shims` in a login profile and `mise activate` in
your interactive startup file, as shown in
[Add shims to PATH](#how-to-add-mise-shims-to-path). Set up activation from
[Shell setup](/shell-setup.html).

### Without activation or shims {#neither-shims-nor-path}

[`mise exec`](/cli/exec.html), [`mise run`](/cli/run.html), and
[`mise en`](/cli/en.html) load tools and environment variables without
changing any startup file:

```sh
mise exec -- node --version
mise run build # runs the task named build
```

This works in CI, scripts, and projects where you do not want to change shell
startup files. It needs `mise` on `PATH`, but no activation or shim directory.

## How shims work {#mise-activate-shims}

mise keeps two shim directories:

- Your user shim directory, `~/.local/share/mise/shims` (`%LOCALAPPDATA%\mise\shims`
  on Windows), set by [`shims_dir`](/configuration/settings.html#shims_dir).
- The system shim directory, `/usr/local/share/mise/shims`, for
  [system installs](/dev-tools/system-installs.html).

When you install a tool, mise adds a shim to the shim directory for every
executable the tool provides. On Unix each shim is a symlink to the `mise`
binary; on Windows it is a small executable by default (see
[`windows_shim_mode`](/configuration/settings.html#windows_shim_mode)).

```sh
mise use node@24 npm:prettier@3
ls -l ~/.local/share/mise/shims/node
# ~/.local/share/mise/shims/node -> ~/.local/bin/mise
~/.local/share/mise/shims/prettier --version
```

A shim covers every installed version of its tool. Each time it runs, it reads
the config for the current directory, picks the version, sets that project's
`[env]` variables, and runs the real executable.

When a shim cannot find the configured version, it installs it, as long as
[`not_found_auto_install`](/configuration/settings.html#not_found_auto_install)
is on. Otherwise it runs the next executable with the same name on `PATH`. That
fallback is convenient for tools you also want outside mise, but for a command
the OS also ships, such as `python3` on Debian or Ubuntu, the shim can silently
run an unrelated system binary. Set
[`not_found_system_fallback`](/configuration/settings.html#not_found_system_fallback)
and `not_found_auto_install` to `false` to make an unresolved shim fail instead.

## Add shims to PATH {#how-to-add-mise-shims-to-path}

`mise activate --shims` prints the lines that put the shim directories and the
[command wrapper](#command-wrappers) directory on `PATH`. Add it to the startup
file that the program you care about reads, keeping your existing setup:

::: code-group

```sh [Bash: ~/.bash_profile]
# Use ~/.bash_login or ~/.profile instead if that is the file you already have.
eval "$(mise activate bash --shims)"
```

```sh [Bash: ~/.bashrc]
eval "$(mise activate bash)"
```

```sh [Zsh: ~/.zprofile]
eval "$(mise activate zsh --shims)"
```

```sh [Zsh: ~/.zshrc]
eval "$(mise activate zsh)"
```

```fish [Fish: ~/.config/fish/config.fish]
if status is-interactive
    mise activate fish | source
else
    mise activate fish --shims | source
end
```

:::

Login shells read the profile files, and interactive shells read `~/.bashrc`
and `~/.zshrc`. A login Bash reads only the first file it finds among
`~/.bash_profile`, `~/.bash_login`, and `~/.profile`, and it reads `~/.bashrc`
only if that file sources it. On Ubuntu and Debian, `~/.profile` usually
sources `~/.bashrc` and sets up `PATH`, so creating a new `~/.bash_profile`
stops `~/.profile` from loading. Add the line to the file you already have, or
make a new `~/.bash_profile` source `~/.profile`:

```sh [~/.bash_profile]
[[ -f ~/.profile ]] && source ~/.profile
eval "$(mise activate bash --shims)"
```

If `mise` is not on `PATH` when the profile runs, call it by its full path,
such as `eval "$($HOME/.local/bin/mise activate zsh --shims)"`, or add the user
shim directory directly. That line leaves out the system shim directory and
command wrappers:

```sh
export PATH="$HOME/.local/share/mise/shims:$PATH"
```

Editors and other GUI programs read these files only when they start, and some
desktop environments read the login profile only when you log in. Restart the
editor, or log out and back in, after the change. See
[IDE integration](/ide-integration.html) for editor-specific setup. A program
started by a scheduler, a service manager, or an IDE that does not read your
profile needs its own `PATH` setting, or can call `mise exec`.

On Windows, editors and other GUI programs do not read `$PROFILE`. Add
`%LOCALAPPDATA%\mise\shims` to your user `Path` as shown in
[Windows shells](/shell-setup.html#windows), and keep `mise activate pwsh` in
`$PROFILE` for PowerShell itself.

## Shims vs PATH activation {#shims-vs-path}

Shims cover the tools themselves. They do not set
[`[env]` variables](#env-vars-and-shims) in your shell, they run only the
install [hooks](#hooks-and-shims), and they make [`which`](#which) print the
shim. Use [PATH activation](#path-activation) for the shell you type into, and
shims for programs that do not start from it.

### PATH activation {#path-activation}

With [`mise activate`](/cli/activate.html), mise adds each selected tool's
`bin` directory to the front of `PATH`:

```sh
echo "$PATH"
# ~/.local/share/mise/installs/python/3.14.8/bin:/usr/local/bin:/usr/bin:/bin
```

mise sets `PATH` and `[env]` variables when the activation script runs, in
bash, zsh, fish, elvish, and PowerShell. Nushell and xonsh wait for the first
prompt or directory change. After that, mise refreshes the environment at each
prompt and, in shells with a [directory-change hook](#hook-on-cd), after each
`cd`. Child processes inherit the result.

While shims are needed for [lazy tools](#lazy-tools) or
`not_found_auto_install`, activation keeps the shim directories on `PATH`
behind the real tool directories. See
[Keep shims out of mise activate](#activate-shims) to remove them.

### Directory changes {#hook-on-cd}

In most shells, mise also updates the environment when the directory changes,
not only when the prompt is drawn, so `cd ~/proj && node -v` uses the project's
version:

| Shell          | Updates on `cd`    | Mechanism                                               |
| -------------- | ------------------ | ------------------------------------------------------- |
| bash           | Yes                | Wraps `cd`, `pushd`, and `popd`, plus `PROMPT_COMMAND`  |
| zsh            | Yes                | `chpwd` hook                                            |
| fish           | Yes                | `--on-variable PWD` handler                             |
| elvish         | Yes                | `after-chdir` hook                                      |
| xonsh          | Yes                | `on_chdir` event                                        |
| PowerShell 7+  | Yes                | `LocationChangedAction`                                 |
| PowerShell 5.x | At the next prompt | Prompt hook only                                        |
| Nushell        | At the next prompt | `env_change.PWD` hook, which Nushell runs at the prompt |

In fish, set `mise_fish_mode` to `eval_after_arrow` to defer the update until
the next command starts, or to `disable_arrow` to update only at the prompt.

::: details Running several commands on one line

In PowerShell 5.x and Nushell, a one-line command keeps the tools of the
directory where the line started:

```sh
cd ~
cd ~/src/proj1 && node -v && cd ~/src/proj2 && node -v
```

Both `node -v` calls use the tools selected for `~`. Shims always resolve from
the current directory, so they handle this line correctly in every shell.

:::

### Environment variables {#env-vars-and-shims}

Reading an `[env]` variable in your shell works only under `mise activate`:

```sh
mise set NODE_ENV=production
echo "$NODE_ENV"
# production
```

This works with either, because `node` runs through its shim:

```sh
mise set NODE_ENV=production
node -p process.env.NODE_ENV
# production
```

`mise exec` and `mise run` load the environment even when a command needs no
mise tool. [Tasks](/tasks/) always run with it:

```sh
mise exec -- bash -c 'echo $NODE_ENV'
# production
```

### Hooks {#hooks-and-shims}

The `cd`, `enter`, and `leave` [hooks](/hooks.html) and
[`watch_files`](/hooks.html#watch-files-hook) run only with `mise activate`.
`preinstall` and `postinstall` run with shims too, because they run during
installation rather than from the shell.

### `which` {#which}

With shims, `which node` prints the shim's path, and `mise which node` prints
the real executable. With PATH activation, `which node` prints the installed
path:

```sh
which node
# ~/.local/share/mise/installs/node/24/bin/node
```

The path can go through a link named after the request, such as `node/24`,
rather than the resolved version. See
[install layout](/dev-tools/install-layout.html) for the experimental layout's
paths.

### Performance {#performance}

PATH activation does its work at prompts and directory changes. Shims resolve
the environment each time a command runs, so a loop that calls a shim pays that
cost on every call. For example, this script resolves the environment 500
times:

```sh [benchmark.sh]
for i in {1..500}; do
    node script.js
done
```

Run it as `mise exec -- bash benchmark.sh` to resolve the environment once.
Child processes then find the real tool directories ahead of the shim
directory. A program launched by a shim passes the resolved environment on to
its children in the same way. See
[slow shell prompts](/troubleshooting.html#slow-shell-prompts) to diagnose
activation overhead.

### Scripts and shell startup files {#using-mise-in-rc-files}

A script never shows a prompt. After activation, it sees the environment that
the activation script applied, plus directory changes in shells with a
[`cd` hook](#hook-on-cd). It does not see config changes made while it runs.
Run `eval "$(mise hook-env -s bash)"` to refresh it, or run the command with
`mise exec`.

A shell startup file such as `~/.zshrc` is a script too. In bash, zsh, and
fish, a tool is available on the line after activation:

::: code-group

```sh [activate]
eval "$(mise activate zsh)"
node some_script.js
```

```sh [shims]
eval "$(mise activate zsh --shims)" # should be first
eval "$(mise activate zsh)"
node some_script.js
```

:::

In Nushell and xonsh, which apply the environment at the first prompt, put the
shims on `PATH` first or use `mise exec`.

## Rebuild shims with mise reshim {#mise-reshim}

mise adds and removes shims when it installs, upgrades, or removes a tool. Run
[`mise reshim`](/cli/reshim.html) when another program adds executables to an
existing installation, for example after a global package install. The Node.js
core tool does this for `npm install -g` through its
[`node.npm_shim`](/configuration/settings.html#node.npm_shim) wrapper; other
package managers have no such hook.

`mise reshim` only adds and removes shims. It does not fix a wrong version or a
broken installation; run it when a command you expect is missing from the shim
directory. `mise reshim --system` rebuilds the system shim directory.

The user shim directory can be a shared directory such as `~/.local/bin`:
`mise reshim` replaces or removes only entries it recognizes as mise shims.
Activation and hook-env treat the whole directory as a shim directory, though,
so use a dedicated [`shims_dir`](/configuration/settings.html#shims_dir) with
activation.

## Exclude commands from shims {#excluding-command-names}

Some commands are also provided by the OS, and other software on the machine
depends on getting the system one. [`shims.exclude`](/configuration/settings.html#shims.exclude)
keeps those names out of the shim directory. mise still installs and manages
the tool; it only skips the shim. This command adds the setting to your global
config:

```sh
mise settings shims.exclude=python,python3,pip,pip3
```

On Arch Linux, for example, `/usr/bin/python` is the distribution's interpreter
and its modules live in a matching `site-packages` directory. Without the
exclusion, entering a project that pins `python` changes which interpreter a
`#!/usr/bin/env python` script gets, and a `PKGBUILD` that calls `python`
during a build picks up the pinned version.

The next `mise reshim` removes excluded names, and lazy-tool bootstrap shims
and plugin-provided shims skip them too. Version-qualified shims stay, so
`python3.12` still resolves to the version a config selects. With no shim,
mise is out of that command's path and no longer loads config each time it
runs.

::: warning
Excluding `python3` means `python3 -m venv` silently builds a virtualenv from
the system interpreter. Use the version-qualified command, such as
`python3.12 -m venv`, to get the mise-managed version. The setting affects only
shims: under `mise activate` without `--shims`, a tool's whole `bin` directory
joins `PATH`, so excluded names are still found there.
:::

## Keep shims out of mise activate {#activate-shims}

PATH activation puts real tool directories first. When the toolset has a
[lazy tool](#lazy-tools) or `not_found_auto_install` is on, it also keeps the
user and system shim directories on `PATH` behind them, so running a missing
tool's command can still install it. To remove the shim directories from
activation entirely, run `mise settings set activate_shims false` and restart
your shell. See [`activate_shims`](/configuration/settings.html#activate_shims)
for what stops working. [Command wrappers](#command-wrappers) keep working,
because they use their own directory, and an explicit `mise activate --shims`
still adds the shims.

## Lazy tools {#lazy-tools}

Set `lazy = true` on a tool to install it the first time one of its commands
runs instead of during `mise install`:

```toml [mise.toml]
[tools]
node = { version = "24", lazy = true }
```

For registry tools, mise creates bootstrap shims from the registry's command
list. A tool with an explicit backend, or one that is not in the registry,
names its commands with `lazy_bins`:

```toml [mise.toml]
[tools]
"github:example/acme" = { version = "1.2.3", lazy = true, lazy_bins = ["acme", "acmectl"] }
```

Running one of a lazy tool's commands installs it, plus any configured tools it
[depends](/dev-tools/#tool-dependencies) on that are missing, and then runs the
command. Nothing else is installed. This works even when
`not_found_auto_install` is off. A lazy declaration never overrides a
higher-precedence config that selects the same tool without `lazy`.

A bare `mise install` skips lazy tools. `mise install --include-lazy` installs
them all now, and `mise install node` installs one. After installation,
`mise activate` puts the real tool ahead of its shim, so later calls cost
nothing extra. `mise activate --shims` keeps dispatching every call through
mise.

Tasks, `mise exec`, and `mise env` add the bootstrap shims behind the tool
directories whenever the toolset has a lazy tool, so a task installs a lazy
tool the first time it runs one of its commands. `mise run` does not install
lazy tools ahead of time. `mise use` rebuilds shims when it changes a lazy
declaration; run `mise reshim` after editing one by hand.

## Command wrappers {#command-wrappers}

A command wrapper keeps a command's name but runs another program. For example,
to have `terraform` run OpenTofu:

```toml [mise.toml]
[tools]
opentofu = "1"

[wrappers]
terraform = "tofu"
```

Run `mise reshim` after adding or removing a wrapper. Wrappers work with both
`mise activate` and `mise activate --shims` and take precedence over an
executable with the same name. If the configured tool that provides the
wrapper's command is missing, the wrapper installs it first, as that command's
own shim would, subject to `not_found_auto_install` unless the tool is lazy.

The table form inserts arguments before the user's own, and can set
environment variables:

```toml [mise.toml]
[tools]
uv = "latest"

[wrappers.python]
command = "uv"
args = ["run", "python"]
```

When Rust comes from rustup or the system rather than mise, this routes every
`cargo` call through [Mr Boxington](https://mr-boxington.jdx.dev/):

```toml [mise.toml]
[tools]
mr-boxington = "latest"

[wrappers.cargo]
command = "mbx"
env = { MBX_CARGO_SHIM_MODE = "1" }
```

When a wrapper runs its command, mise removes the shim and wrapper directories
from `PATH`, so `mbx` finds the real Cargo: mise-managed Rust when configured,
otherwise rustup or the system installation. For mise-managed Rust, the
[Rust guide](/lang/rust.html#share-cargo-builds-with-mr-boxington) sets this up
with the `mr_boxington` tool option and the `mr-boxington` tool.
