# Shims

There are several ways to load the `mise` context (dev tools, environment variables) into your shell:

- `mise activate` (also called ["mise PATH activation"](#path-activation)), where `mise` updates your `PATH` and other environment variables every time your prompt is displayed.
- [`mise activate --shims`](#mise-activate-shims), which uses shims to load dev tools.
- Using [`mise x|exec`](/cli/exec) or [`mise r|run`](/cli/run) for ad-hoc commands or tasks (see ["neither shims nor PATH"](#neither-shims-nor-path)).

This page explains the differences between these methods and how to use them. In particular, it will help you decide whether to use shims or `mise activate` in your shell.

## Overview of the `mise` activation methods {#overview}

### PATH activation {#path-activation}

mise's "PATH" activation method updates environment variables every time the prompt is displayed. In particular, it updates the `PATH` environment variable, which your shell uses to search for the programs it can run.

::: info
This is the method used when you add the `echo 'eval "$(mise activate bash)"' >> ~/.bashrc` line to your shell rc file (in this case, for bash).
:::

For example, by default, your `PATH` variable might look like this:

```sh
echo $PATH
/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

With [`mise activate`](/cli/activate.html), `mise` automatically adds the required tools to `PATH`.

```sh
PATH="$HOME/.local/share/mise/installs/python/3.15.0/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
```

In this example, the python `bin` directory was added at the beginning of `PATH`, making it available in the current shell session.
When a fuzzy version like `python = "3.15"` or `node = "26"` is active, this path may use the requested-version symlink, such as `~/.local/share/mise/installs/python/3.15/bin`, instead of the fully resolved patch version.

While `PATH` activation works well in most cases, `shims` are preferable in some situations, such as when you are not using an interactive shell (for example, when using `mise` in an IDE or a script).

### Shims {#mise-activate-shims}

::: warning
`mise activate --shims` does not support all the features of `mise activate`.<br>
See [shims vs path](/dev-tools/shims.html#shims-vs-path) for more information.
:::

When using shims, `mise` places small executables (`shims`) in a directory that is included in your `PATH`. You can think of `shims` as symlinks to the mise binary that intercept commands and load the appropriate context.

```sh
ls -l ~/.local/share/mise/shims/node
# [...] ~/.local/share/mise/shims/node -> ~/.local/bin/mise
```

By default, the shim directory is located at `~/.local/share/mise/shims` (on Windows: `%LOCALAPPDATA%\mise\shims`). When you install a tool (for example, `node`), `mise` adds an entry to the `shims` directory for every binary the tool provides (for example, `~/.local/share/mise/shims/node`).

```sh
mise use -g node@20
npm install -g prettier@3.1.0

~/.local/share/mise/shims/node -v
# v20.0.0
~/.local/share/mise/shims/prettier -v
# 3.1.0
```

Rather than calling `~/.local/share/mise/shims/node` directly, you can add the `shims` directory to your `PATH`.

```sh
export PATH="$HOME/.local/share/mise/shims:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
```

This makes all dev tools available in your current shell session as well as in non-interactive environments.

## Lazy tools

Set `lazy = true` on a tool when it should be installed the first time one of
its commands is invoked instead of by a bare `mise install`:

```toml
[tools]
node = { version = "24", lazy = true }
```

For registry shorthands, mise creates bootstrap shims from the registry's
`bins` metadata. Explicit backends and tools that are not in the registry must
declare their command names with `lazy_bins`:

```toml
[tools]
"github:example/acme" = { version = "1.2.3", lazy = true, lazy_bins = ["acme", "acmectl"] }
```

Run `mise reshim` after editing a lazy declaration directly. Commands such as
`mise use` that update tool configuration rebuild the shim farm automatically.
Invoking a lazy shim installs only its configured provider and then executes
it. This is independent of `not_found_auto_install`; an explicit project tool
selection is never bypassed by a lower-precedence lazy declaration.

A bare `mise install` skips missing lazy tools. Pass `--include-lazy` to install
all configured tools, including lazy declarations, or name one explicitly, such
as `mise install node`, to install only that lazy tool immediately. Once
installed, normal `mise activate` places the real tool path ahead of the shim
farms, so later calls have no shim dispatch overhead. `mise activate --shims`
remains project-aware and dispatches every call through mise by design.

Tasks and `mise x` work the same way. `mise run` does not preinstall lazy tools.
Instead, whenever the toolset has a lazy declaration, the environment mise
builds for a task (and for `mise x` and `mise env`) places the shim farms behind
the tool paths if they are not already on PATH, and any missing bootstrap shims
are created first. The task installs the tool the first time it runs one of its
commands. `mise x -- <command>` installs the provider of a lazy command directly.

::: tip
[`mise activate --shims`](/cli/activate.html#shims) is a shorthand for adding the shims directory to PATH.
:::

## How to add mise shims to PATH

The recommended way to add `shims` to `PATH` is to call [`mise activate --shims`](/cli/activate.html#shims) in one of your shell initialization files. For example:

::: code-group

```sh [bash]
# note that bash will read from ~/.profile or ~/.bash_profile if the latter exists
# ergo, you may want to check to see which is defined on your system and only append to the existing file
echo 'eval "$(mise activate bash --shims)"' >> ~/.bash_profile # this sets up non-interactive sessions
echo 'eval "$(mise activate bash)"' >> ~/.bashrc       # this sets up interactive sessions
```

```sh [zsh]
echo 'eval "$(mise activate zsh --shims)"' >> ~/.zprofile # this sets up non-interactive sessions
echo 'eval "$(mise activate zsh)"' >> ~/.zshrc    # this sets up interactive sessions
```

```sh [fish]
echo 'mise activate fish --shims | source' >> ~/.config/fish/config.fish
echo 'mise activate fish | source' >> ~/.config/fish/config.fish
```

:::

In this example, we use [`mise activate --shims`](/cli/activate.html#shims) in the non-interactive shell configuration file (like `.bash_profile` or `.zprofile`) and `mise activate` in the interactive shell configuration file (like `.bashrc` or `.zshrc`).

::: info
It's fine to call [`mise activate --shims`](/cli/activate.html#shims) in your shell profile file and then
later call [`mise activate`](/cli/activate.html) in an interactive session. PATH
activation keeps the user and existing system shim farms behind real tool paths
when the effective toolset contains a lazy declaration or
`not_found_auto_install` is enabled. Without either, full activation
removes the shim farms as before. This makes lazy bootstrap commands available
without adding dispatch overhead after installation. `not_found_auto_install`
still controls general missing-tool installation, but does not disable an
explicit `lazy = true` declaration.

:::

::: info
When a shim cannot resolve a mise-managed tool (for example, a version pinned in `mise.toml` that hasn't
been installed and [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) is
disabled), it falls back to the first same-named executable found elsewhere on `PATH` rather than erroring.
This is convenient for tools you also want available outside of mise, but for a tool the OS also ships
(`python3` on Debian/Ubuntu, for example) it means the shim can silently run a completely different,
unrelated binary instead of failing loudly.

Set [`not_found_system_fallback`](/configuration/settings.html#not_found_system_fallback) to `false`,
alongside `not_found_auto_install = false`, if you'd rather an unresolvable shim fail outright.
:::

- You can also decide to use only `shims` if you prefer, though this comes with some [limitations](/dev-tools/shims.html#shims-vs-path).
- An alternative to [`mise activate --shims`](/cli/activate.html#shims) is to use `export PATH="$HOME/.local/share/mise/shims:$PATH"`. This can be helpful if `mise` is not yet available at that point.

### mise reshim

To force `mise` to update the contents of the `shims` directory, run `mise reshim`.

Use `mise reshim --system` for the system shim farm. If `shims_dir` and
`system_shims_dir` resolve to the same physical path, either command reconciles
one combined farm containing both scopes.

`mise` already reshims whenever a tool is installed, updated, or removed, so you don't need to run it in those cases. A reshim also happens by default when using most tools, such as `npm`.

`mise reshim` only creates and removes shims. Some users treat it as a
"fix it" button, but it is only necessary when `~/.local/share/mise/shims` doesn't contain something it should.

For `mise reshim`, the configured shim directory may be a shared executable directory such as
`~/.local/bin` or `/usr/local/bin`: reshim only replaces or removes entries it recognizes as mise
shims, and leaves a same-named unmanaged file in place. Other mise features still identify shim
directories as whole `PATH` entries, however, so a shared directory is not yet supported with
`mise activate`, hook-env, or internal dependency lookups. Use a dedicated `shims_dir` if you use
those features.

## Command wrappers

Use `[wrappers]` when a command should always pass through another program while
keeping its ordinary name. For example, this routes every `cargo` invocation
through [Mr Boxington](https://github.com/jdx/mr-boxington):

```toml
[tools]
mr-boxington = "1.4.1"

[wrappers.cargo]
command = "mbx"
env = { MBX_CARGO_SHIM_MODE = "1" }
```

Run `mise reshim` after adding or removing a wrapper. The wrapper is available
with both `mise activate` and `mise activate --shims`, and takes precedence over
an executable with the same name. When it delegates, mise removes its dispatch
directories from `PATH`, so `mbx` resolves Cargo from mise-managed Rust when
configured and otherwise falls through to rustup or the system installation.

A short form is available when no arguments or environment variables are needed:

```toml
[wrappers]
terraform = "tofu"
```

The detailed form can insert arguments before those supplied by the user:

```toml
[wrappers.python]
command = "uv"
args = ["run", "python"]
```

## Shims vs PATH {#shims-vs-path}

The following features are affected when shims are used **instead** of [PATH activation](#path-activation):

- [Env vars](/environments/) defined in mise are only available to mise tools
- Most [hooks](/hooks.html) won't trigger
- The Unix `which` command points to the shim, obscuring the real executable

In general, PATH activation (`mise activate`) is recommended over shims for _interactive_ situations.

With `activate`, every time the prompt is displayed, mise determines what `PATH` and other
env vars should be and exports them. This is why it doesn't work well for non-interactive situations like scripts: the prompt is never displayed, so you have to call `mise hook-env` manually to get mise to update
the env vars (though there are exceptions; see [hook on `cd`](#hook-on-cd)).

### Env vars and shims

A downside of shims is that environment variables are only loaded when a shim is called. This means that if you
set an [environment variable](/environments/) in `mise.toml`, it is only applied when a shim is called.

The following example only works under `mise activate`:

```sh
$ mise set NODE_ENV=production
$ echo $NODE_ENV
production
```

But this works with either:

```sh
$ mise set NODE_ENV=production
$ node -p process.env.NODE_ENV
production
```

You can also use [`mise x|exec`](/cli/exec.html) and [`mise r|run`](/cli/run.html) to load the environment even if you don't need any mise tools:

```sh
$ mise set NODE_ENV=production
$ mise x -- bash -c "echo \$NODE_ENV"
production
$ mise r some_task_that_uses_NODE_ENV
production
```

::: tip
In general, [tasks](/tasks/) are a good way to ensure that the mise environment is always loaded.
:::

### Hooks and shims

The [hooks](/hooks.html) `cd`, `enter`, and `leave` only trigger with `mise activate`. The separate [`watch_files`](/hooks.html#watch-files-hook) configuration also requires `mise activate`. However, `preinstall` and `postinstall` still work with shims because they don't require shell integration.

### `which`

Many users find `which` valuable. Shims effectively "break" `which`, causing it to show the location of the shim. A workaround is `mise which`, which shows the actual location. Some users prefer the "cleanliness" of running `which node` and getting back a real path with a version number in it, e.g.:

```sh
$ which node
~/.mise/installs/node/20/bin/node
```

### Performance

Truthfully, you're unlikely to notice a performance difference between shims and `mise activate`.

- With `mise activate`, mise runs every time the prompt is displayed, so you pay a few ms
  every time the prompt is displayed. You pay that penalty every time you run any command, regardless
  of whether it uses a mise tool. mise has some short-circuiting logic to make it faster
  when nothing has changed, but it doesn't help much unless you have a very complex setup.
- Shims have the same performance profile but run when the shim is called. This makes some situations
  better and some worse.

If you are calling a shim from within a bash script like this:

```sh
for i in {1..500}; do
    node script.js
done
```

You'll pay the mise penalty every time you call it within the loop. However, if you instead
call a subprocess from within a shim (say, node spawning a node subprocess), you will _not_ pay a new
penalty. This is because when a shim is called, mise sets up `PATH` for all tools, and
those `PATH` entries come before the shim directory.

In other words, which is faster depends on how you're calling mise. Realistically,
though, most users will not notice the few ms of lag `mise activate` adds to their terminal.
See [Troubleshooting: Slow shell prompts](/troubleshooting.html#slow-shell-prompts) for how to diagnose performance issues.

The only difference between `hook-env` and shims is that with `hook-env` you need to call
it again when you change directories, whereas with shims that isn't necessary. If you use both, `mise activate`
takes care of the shim farms for you: they are kept behind the tool paths as a fallback. Disabling
[`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) disables general missing-tool
installation, but explicit `lazy = true` declarations remain available through their shims.

## Neither shims nor PATH {#neither-shims-nor-path}

There are many ways to load the mise environment that don't require either, chiefly:
[`mise x|exec`](/cli/exec.html), [`mise r|run`](/cli/run.html), or [`mise en`](/cli/en.html).

These all load the tools and env vars before executing something. This might
be ideal because you don't need to modify your shell rc file at all and the environment is always loaded
explicitly. Some may find this a "clean" way of working.

The obvious downside is that you need to prefix every command with `mise exec|run`, though you can easily alias these to `mx|mr`.

- This approach suits people who prefer precision over convenience.
- It also suits those who only want to use mise on a single project because that's what their team uses, and
  prefer not to manage anything else on their system with it. A shell extension
  would be overkill for that use case.

## Hook on `cd` {#hook-on-cd}

For some shells (`bash`, `zsh`, `fish`, `xonsh`), `mise` hooks into the `cd` command, while in others, it only runs when the prompt is displayed. This relies on `chpwd` in `zsh`, `PROMPT_COMMAND` in `bash`, `fish_prompt` in `fish`, and `on_chdir` in `xonsh`.

The upside is that it doesn't run as frequently, but since mise is written in Rust, the cost of executing
mise is negligible (a few ms).

::: details Running several commands in a single line

If you run a set of commands in a single line like the following:

```sh
cd ~
cd ~/src/proj1 && node -v && cd ~/src/proj2 && node -v
```

With `mise activate` in a shell without a `cd` hook, this uses the tools from `~`, not from `~/src/proj1` or `~/src/proj2`, even after the directory changes.

This is because in these shells `mise` runs just before your prompt is displayed, whereas in others it hooks into `cd`. Shims _will_ always work with the inline example above.

:::

## Using mise in rc files

rc files like `.zshrc` are unusual: they are scripts, but they run only for interactive sessions. If you need
to access tools provided by mise inside an rc file, you have two options:

::: code-group

```sh [hook-env]
eval "$(mise activate zsh)"
eval "$(mise hook-env -s zsh)"
node some_script.js
```

```sh [shims]
eval "$(mise activate zsh --shims)" # should be first
eval "$(mise activate zsh)"
node some_script.js
```

:::
