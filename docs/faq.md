# FAQs

## I don't want to put a `mise.toml`/`.tool-versions` file into my project since git shows it as an untracked file

Use [`mise.local.toml`](https://mise.jdx.dev/configuration.html#mise-toml) and put that into your global gitignore file. This file should never be committed.

If you really want to use a `mise.toml` or `.tool-versions`, here are 3 ways to make git ignore these files:

- Adding `mise.toml` to project's `.git/info/exclude`. This file is local to your project so
  there is no need to commit it.
- Adding `mise.toml` to project's `.gitignore` file. This has the downside that you need to
  commit the change to the ignore file.
- Adding `mise.toml` to global gitignore (`core.excludesFile`). This will cause git to
  ignore `mise.toml` files in all projects. You can explicitly add one to a project if needed
  with `git add --force mise.toml`.

## What is the difference between "nodejs" and "node" (or "golang" and "go")?

These are aliased. For example, `mise use nodejs@14.0` is the same as `mise install node@14.0`. This
means it is not possible to have these be different plugins.

This is for convenience so you don't need to remember which one is the "official" name. However if
something with the aliasing is acting up, submit a ticket or just stick to using "node" and "go".
Under the hood, when mise reads a config file or takes CLI input it will swap out "nodejs" and
"golang".

While this change is rolling out, there is some migration code that will move installs/plugins from
the "nodejs" and "golang" directories to the new names. If this runs for you you'll see a message
but it should not run again unless there is some kind of problem. In this case, it's probably
easiest to just
run
`rm -rf ~/.local/share/mise/installs/{golang,nodejs} ~/.local/share/mise/plugins/{golang,nodejs}`.

Once most users have migrated over this migration code will be removed.

## What does `mise activate` do?

It registers a shell hook to run `mise hook-env` every time the shell prompt is displayed.
`mise hook-env` checks the current env vars (most importantly `PATH` but there are others like
`GOROOT` or `JAVA_HOME` for some tools) and adds/removes/updates the ones that have changed.

For example, if you `cd` into a different directory that has `java 18` instead of `java 17`
specified, just before the next prompt is displayed the shell runs: `eval "$(mise hook-env)"`
which will execute something like this in the current shell session:

```sh
export JAVA_HOME=$HOME/.local/share/installs/java/18
export PATH=$HOME/.local/share/installs/java/18/bin:$PATH
```

In reality updating `PATH` is a bit more complex than that because it also needs to remove java-17,
but you get the idea.

You may think that is excessive to run `mise hook-env` every time the prompt is displayed
and it should only run on `cd`, however there are plenty of
situations where it needs to run without the directory changing, for example if `.tool-versions` or
`mise.toml` was just edited in the current shell.

Because it runs on prompt display, if you attempt to use `mise activate` in a
non-interactive session (like a bash script), it will never call `mise hook-env` and in effect will
never modify PATH because it never displays a prompt. For this type of setup, you can either call
`mise hook-env` manually every time you wish to update PATH, or use [shims](/dev-tools/shims.md)
instead (preferred).
Or if you only need to use mise for certain commands, just prefix the commands with
[`mise x --`](./cli/exec).
For example, `mise x -- npm test` or `mise x -- ./my_script.sh`.

`mise hook-env` will exit early in different situations if no changes have been made. This prevents
adding latency to your shell prompt every time you run a command. You can run `mise hook-env`
yourself
to see what it outputs, however it is likely nothing if you're in a shell that has already been
activated.

`mise activate` also creates a shell function (in most shells) called `mise`.
This is a trick that makes it possible for `mise shell`
and `mise deactivate` to work without wrapping them in `eval "$(mise shell)"`.

## Windows support?

While mise runs great in WSL, native Windows is also supported, though via the use of shims until
someone adds [powershell](https://github.com/jdx/mise/issues/3451) support.

As you'll need to use shims, this means you won't have environment variables from mise.toml unless you run mise via
[`mise x`](/cli/exec) or [`mise run`](/cli/run)—though that's actually how I use mise on my mac so
for me that's my preferred workflow anyway.

## How do I use mise with http proxies?

Short answer: just set `http_proxy` and `https_proxy` environment variables. These should be
lowercase.

This may not work with all plugins if they are not configured to use these env vars.
If you're having a proxy-related issue installing something specific you should post an issue on the
plugin's repository.

## How do the shorthand plugin names map to repositories?

e.g.: how does `mise plugin install elixir` know to fetch <https://github.com/asdf-vm/asdf-elixir>?

We maintain [an index](https://github.com/mise-plugins/registry) of shorthands that mise uses as a
base.
This is regularly updated every time that mise has a release. This repository is stored directly
into
the codebase in [registry.toml](https://github.com/jdx/mise/blob/main/registry.toml).

## Does "node@20" mean the newest available version of node?

It depends on the command. Normally, for most commands and inside of config files, "node@20" will
point to the latest _installed_ version of node-20.x. You can find this version by running
`mise latest --installed node@20` or by seeing what the `~/.local/share/mise/installs/node/20`
symlink
points to:

```sh
$ ls -l ~/.local/share/mise/installs/node/20
[...] /home/jdx/.local/share/mise/installs/node/20 -> node-v20.0.0-linux-x64
```

There are some exceptions to this, such as the following:

- `mise install node@20`
- `mise latest node@20`
- `mise upgrade node@20`

These will use the latest _available_ version of node-20.x. This generally makes sense because you
wouldn't want to install a version that is already installed.

## How do I migrate from asdf?

- Install mise and set up `mise activate` as described in the [getting started guide](/getting-started)
- remove asdf from your shell rc file
- Run `mise install` in a directory with an asdf `.tool-versions` file and mise will install the tools

Note that `mise` does not consider `~/.tool-versions` files to be a global config file like `asdf` does. `mise` uses a
`~/.config/mise/config.toml` file for global configuration.

Here is an example script you can use to migrate your global `.tool-versions` file to mise:

```shell
mv ~/.tool-versions ~/.tool-versions.bak
cat ~/.tool-versions.bak | tr -s ' ' | tr ' ' '@' | xargs -n2 mise use -g
```

Once you are comfortable with mise, you can remove the `.tool-versions.bak` file and [uninstall `asdf`](https://asdf-vm.com/manage/core.html#uninstall)

## How compatible is mise with asdf?

mise should be able to read/install any `.tool-versions` file used by asdf. Any asdf plugin
should be usable in mise. The commands in mise are slightly
different, such as `mise install node@20.0.0` vs `asdf install node 20.0.0`—this is done so
multiple tools can be specified at once. However, asdf-style syntax is still supported: (`mise
install node 20.0.0`). This is the case for most commands, though the help for the command may
say that asdf-style syntax is supported. When in doubt, just try asdf syntax and see if it works—it probably does.

UPDATE (2025-01-01): mise was designed to be compatible with the asdf written in bash (<=0.15). The new asdf written in go (>=0.16)
has commands mise does not support like `asdf set`. `mise set` is an existing command that is completely different than `asdf set`—in mise that sets env vars.

This isn't important for usability reasons so much as making it so plugins continue to work that
call asdf commands inside of the plugin code.

Using commands like `mise use` may output `.tool-versions` files that are not compatible with asdf,
such as using fuzzy versions. You can set `--pin` or `MISE_PIN=1` to make `mise use` output asdf-compatible versions
in `.tool-versions`. Alternatively, you can have `mise.toml` and `.tool-versions` sitting side-by-side. `mise.toml` tools
will override tools defined in a `.tool-versions` in the same directory.

That said, in general compatibility with asdf is no longer a design goal. It's long been the case
that there is no reason to prefer asdf to mise so users should migrate. While plenty of users have
teams which use both in tandem, issues with such a setup are unlikely to be prioritized.

## How do I disable/force CLI color output?

mise uses [console.rs](https://docs.rs/console/latest/console/fn.colors_enabled.html) which
honors the [clicolors spec](https://bixense.com/clicolors/):

- `CLICOLOR != 0`: ANSI colors are supported and should be used when the program isn’t piped.
- `CLICOLOR == 0`: Don’t output ANSI color escape codes.
- `CLICOLOR_FORCE != 0`: ANSI colors should be enabled no matter what.

## Is mise secure?

Providing a secure supply chain is incredibly important. mise already provides a more secure
experience when compared to asdf. Security-oriented evaluations and contributions are welcome.
We also urge users to look after the plugins they use, and urge plugin authors to look after
the users they serve.

For more details see [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).

## What is usage?

usage (<https://usage.jdx.dev/>) is a spec and CLI for defining CLI tools.

Arguments, flags, environment variables, and config files can all be defined in a Usage spec. It can be thought of like OpenAPI (swagger) for CLIs.

`usage` can be installed with `mise` using `mise use -g usage` and is required to get the autocompletion working. See [autocompletion](/installing-mise.html#autocompletion).

You can leverage usage in file tasks to get auto-completion working, see [file tasks arguments](/tasks/file-tasks.html#arguments).

## VSCode for windows extension with error `spawn EINVAL`

In VSCode, many extensions will throw an "error spawn EINVAL" due to a [Node.js security fix](https://nodejs.org/en/blog/vulnerability/april-2024-security-releases-2#command-injection-via-args-parameter-of-child_processspawn-without-shell-option-enabled-on-windows-cve-2024-27980---high).

You can change [windows_shim_mode](https://mise.jdx.dev/configuration/settings.html#windows_shim_mode) to `hardlink` or `symlink`
