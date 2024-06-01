# FAQs

### I don't want to put a `.tool-versions` file into my project since git shows it as an untracked file

You can make git ignore these files in 3 different ways:

- Adding `.tool-versions` to project's `.gitignore` file. This has the downside that you need to commit the change to the ignore file.
- Adding `.tool-versions` to project's `.git/info/exclude`. This file is local to your project so there is no need to commit it.
- Adding `.tool-versions` to global gitignore (`core.excludesFile`). This will cause git to ignore `.tool-versions` files in all projects. You can explicitly add one to a project if needed with `git add --force .tool-versions`.

### What is the difference between "nodejs" and "node" (or "golang" and "go")?

These are aliased. For example, `mise use nodejs@14.0` is the same as `mise install node@14.0`. This
means it is not possible to have these be different plugins.

This is for convenience so you don't need to remember which one is the "official" name. However if
something with the aliasing is acting up, submit a ticket or just stick to using "node" and "go".
Under the hood, when mise reads a config file or takes CLI input it will swap out "nodejs" and
"golang".

While this change is rolling out, there is some migration code that will move installs/plugins from
the "nodejs" and "golang" directories to the new names. If this runs for you you'll see a message
but it should not run again unless there is some kind of problem. In this case, it's probably
easiest to just run `rm -rf ~/.local/share/mise/installs/{golang,nodejs} ~/.local/share/mise/plugins/{golang,nodejs}`.

Once most users have migrated over this migration code will be removed.

### What does `mise activate` do?

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
`.mise.toml` was just edited in the current shell.

Because it runs on prompt display, if you attempt to use `mise activate` in a
non-interactive session (like a bash script), it will never call `mise hook-env` and in effect will
never modify PATH because it never displays a prompt. For this type of setup, you can either call
`mise hook-env` manually every time you wish to update PATH, or use [shims](/dev-tools/shims.md) instead (preferred).
Or if you only need to use mise for certain commands, just prefix the commands with
[`mise x --`](./cli/#mise-exec-options-tool-version-command).
For example, `mise x -- npm test` or `mise x -- ./my_script.sh`.

`mise hook-env` will exit early in different situations if no changes have been made. This prevents
adding latency to your shell prompt every time you run a command. You can run `mise hook-env` yourself
to see what it outputs, however it is likely nothing if you're in a shell that has already been activated.

`mise activate` also creates a shell function (in most shells) called `mise`.
This is a trick that makes it possible for `mise shell`
and `mise deactivate` to work without wrapping them in `eval "$(mise shell)"`.

### `mise activate` doesn't work in `~/.profile`, `~/.bash_profile`, `~/.zprofile`

`mise activate` should only be used in `rc` files. These are the interactive ones used when
a real user is using the terminal. (As opposed to being executed by an IDE or something). The prompt
isn't displayed in non-interactive environments so PATH won't be modified.

For non-interactive setups, consider using shims instead which will route calls to the correct
directory by looking at `PWD` every time they're executed. You can also call `mise exec` instead of
expecting things to be directly on PATH. You can also run `mise env` in a non-interactive shell, however that
will only setup the global tools. It won't modify the environment variables when entering into a
different project.

Also see the [shebang](/tips-and-tricks#shebang) example for a way to make scripts call mise to get the runtime.
That is another way to use mise without activation.

### mise is failing or not working right

First try setting `MISE_DEBUG=1` or `MISE_TRACE=1` and see if that gives you more information.
You can also set `MISE_LOG_FILE_LEVEL=debug MISE_LOG_FILE=/path/to/logfile` to write logs to a file.

If something is happening with the activate hook, you can try disabling it and calling `eval "$(mise hook-env)"` manually.
It can also be helpful to use `mise env` which will just output environment variables that would be set.
Also consider using [shims](/dev-tools/shims.md) which can be more compatible.

If runtime installation isn't working right, try using the `--raw` flag which will install things in
series and connect stdin/stdout/stderr directly to the terminal. If a plugin is trying to interact
with you for some reason this will make it work.

Of course check the version of mise with `mise --version` and make sure it is the latest. Use `mise self-update`
to update it. `mise cache clean` can be used to wipe the internal cache and `mise implode` can be used
to remove everything except config.

Before submitting a ticket, it's a good idea to test what you were doing with asdf. That way we can rule
out if the issue is with mise or if it's with a particular plugin. For example, if `mise install python@latest`
doesn't work, try running `asdf install python latest` to see if it's an issue with asdf-python.

Lastly, there is `mise doctor` which will show diagnostic information and any warnings about issues
detected with your setup. If you submit a bug report, please include the output of `mise doctor`.

### New version of a tool is not available

There are 2 places that versions are cached so a brand new release might not appear right away.

The first is that the mise CLI caches versions for 24 hours. This can be cleared with `mise cache clear`.

The second uses the mise-versions.jdx.dev host as a centralized
place to list all of the versions of most plugins. This is intended to speed up mise and also
get around GitHub rate limits when querying for new versions. Check that repo for your plugin to
see if it has an updated version. This service can be disabled by setting `MISE_USE_VERSIONS_HOST=0`.

### Windows support?

This is something we'd like to add! <https://github.com/jdx/mise/discussions/66>

It's not a near-term goal and it would require plugin modifications, but it should be feasible.

### How do I use mise with http proxies?

Short answer: just set `http_proxy` and `https_proxy` environment variables. These should be lowercase.

This may not work with all plugins if they are not configured to use these env vars.
If you're having a proxy-related issue installing something specific you should post an issue on the plugin's repository.

### How do the shorthand plugin names map to repositories?

e.g.: how does `mise plugin install elixir` know to fetch <https://github.com/asdf-vm/asdf-elixir>?

We maintain [an index](https://github.com/rtx-plugins/registry) of shorthands that mise uses as a base.
This is regularly updated every time that mise has a release. This repository is stored directly into
the codebase [here](https://github.com/jdx/mise/blob/main/src/default_shorthands.rs).

### Does "node@20" mean the newest available version of node?

It depends on the command. Normally, for most commands and inside of config files, "node@20" will
point to the latest _installed_ version of node-20.x. You can find this version by running
`mise latest --installed node@20` or by seeing what the `~/.local/share/mise/installs/node/20` symlink
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

### How do I migrate from asdf?

First, just install mise with `mise activate` like in the getting started guide and remove asdf from your
shell rc file.

Then you can just run `mise install` in a directory with an asdf `.tool-versions` file and it will
install the runtimes. You could attempt to avoid this by copying the internal directory from asdf over
to mise with `cp -r ~/.asdf ~/.local/share/mise`. That _should_ work because they use the same structure,
however this isn't officially supported or regularly tested. Alternatively you can set `MISE_DATA_DIR=~/.asdf`
and see what happens.

### How compatible is mise with asdf?

mise should be able to read/install any `.tool-versions` file used by asdf. Any asdf plugin
should be usable in mise. The commands in mise are slightly
different, such as `mise install node@20.0.0` vs `asdf install node 20.0.0`—this is done so
multiple tools can be specified at once. However, asdf-style syntax is still supported: (`mise
install node 20.0.0`). This is the case for most commands, though the help for the command may
say that asdf-style syntax is supported.

When in doubt, just try asdf syntax and see if it works. If it doesn't open a ticket. It may
not be possible to support every command identically, but
we should attempt to make things as consistent as possible.

This isn't important for usability reasons so much as making it so plugins continue to work that
call asdf commands.

If you need to switch to/from asdf or work in a project with asdf users, you can set
[`MISE_ASDF_COMPAT=1`](/configuration#mise_asdf_compat1). That prevents
mise from writing `.tool-versions` files that will not be
compatible with asdf. Also consider using `.mise.toml` instead which won't conflict with asdf setups.

### mise isn't working when calling from tmux or another shell initialization script

`mise activate` will not update PATH until the shell prompt is displayed. So if you need to access a
tool provided by mise before the prompt is displayed you can either
[add the shims to your PATH](getting-started.html#2-add-mise-shims-to-path) e.g.

```bash
export PATH="$HOME/.local/share/mise/shims:$PATH"
python --version # will work after adding shims to PATH
```

Or you can manually call `hook-env`:

```bash
eval "$(mise activate bash)"
eval "$(mise hook-env)"
python --version # will work only after calling hook-env explicitly
```

For more information, see [What does `mise activate` do?](#what-does-mise-activate-do)

### How do I disable/force CLI color output?

mise uses [console.rs](https://docs.rs/console/latest/console/fn.colors_enabled.html) which
honors the [clicolors spec](https://bixense.com/clicolors/):

- `CLICOLOR != 0`: ANSI colors are supported and should be used when the program isn’t piped.
- `CLICOLOR == 0`: Don’t output ANSI color escape codes.
- `CLICOLOR_FORCE != 0`: ANSI colors should be enabled no matter what.

### Is mise secure?

Providing a secure supply chain is incredibly important. mise already provides a more secure
experience when compared to asdf. Security-oriented evaluations and contributions are welcome.
We also urge users to look after the plugins they use, and urge plugin authors to look after
the users they serve.

For more details see [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).
