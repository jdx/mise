# Shims

There are several ways for the `mise` context (dev tools, environment variables) to be loaded into your shell:

- `mise activate` (also called ["mise PATH activation"](#path-activation)) where `mise` updates your `PATH` and other environment variables every time your prompt is displayed.
- [`mise activate --shims`](#mise-activate-shims) which uses shims to load dev tools.
- Using [`mise x|exec`](/cli/exec) or [`mise r|run`](/cli/run) for ad-hoc commands or tasks (see ["neither shims nor PATH"](#neither-shims-nor-path)).

This page will help you understand the differences between these methods and how to use them. In particular, it will help you decide if you should use shims or `mise activate` in your shell.

## Overview of the `mise` activation methods {#overview}

### PATH activation {#path-activation}

Mise's "PATH" activation method updates environment variables every time the prompt is displayed. In particular, it updates the `PATH` environment variable, which is used by your shell to search for the programs it can run.

::: info
This is the method used when you add the `echo 'eval "$(mise activate bash)"' >> ~/.bashrc` line to your shell rc file (in this case, for bash).
:::

For example, by default, your `PATH` variable might look like this:

```sh
echo $PATH
/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin
```

If using [`mise activate`](/cli/activate.html), `mise` will automatically add the required tools to `PATH`.

```sh
PATH="$HOME/.local/share/mise/installs/python/3.13.0/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
```

In this example, the python `bin` directory was added at the beginning of the `PATH`, making it available in the current shell session.

While the `PATH` design of `mise` works great in most cases, there are some situations where `shims` are preferable. This is the case when you are not using an interactive shell (for example, when using `mise` in an IDE or a script).

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

By default, the shim directory is located at `~/.local/share/mise/shims`. When installing a tool (for example, `node`), `mise` will add some entries for every binary provided by this tool in the `shims` directory (for example, `~/.local/share/mise/shims/node`).

```sh
mise use -g node@20
npm install -g prettier@3.1.0

~/.local/share/mise/shims/node -v
# v20.0.0
~/.local/share/mise/shims/prettier -v
# 3.1.0
```

To avoid calling `~/.local/share/mise/shims/node`, you can add the `shims` directory to your `PATH`.

```sh
export PATH="$HOME/.local/share/mise/shims:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
```

This will effectively make all dev tools available in your current shell session as well as non-interactive environments.

::: tip
[`mise activate --shims`](/cli/activate.html#shims) is a shorthand for adding the shims directory to PATH.
:::

## How to add mise shims to PATH

The recommended way to add `shims` to `PATH` is to call [`mise activate --shims`](/cli/activate.html#shims) in one of your shell initialization file. For example, you can do the following:

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
echo 'mise activate fish | source' >> ~/.config/fish/fish.config
```

:::

In this example, we use [`mise activate --shims`](/cli/activate.html#shims) in the non-interactive shell configuration file (like `.bash_profile` or `.zprofile`) and `mise activate` in the interactive shell configuration file (like `.bashrc` or `.zshrc`)

::: info
[`mise activate`](/cli/activate.html) will remove the shims directory from the `PATH` so it's fine
to call [`mise activate --shims`](/cli/activate.html#shims) in your shell profile file then later call `mise activate` in an interactive session.
:::

- You can also decide to use only `shims` if you prefer, though this comes with some [limitations](/dev-tools/shims.html#shims-vs-path).
- An alternative to [`mise activate --shims`](/cli/activate.html#shims) is to use `export PATH="$HOME/.local/share/mise/shims:$PATH"`. This can be helpful if `mise` is not yet available at that point in time.

### mise reshim

To force `mise` to update the content of the `shims` directory, you can manually call `mise reshim`.

Note that `mise` already runs a reshim anytime a tool is installed/updated/removed, so you don't need to use it for those scenarios. It is also done by default when using most tools such as `npm`.

`mise reshim` only creates/removes the shims. Some users sometimes use it as a
"fix it" button, but it is only necessary if `~/.local/share/mise/shims` doesn't contain something it should.

Do not add additional executable in the `mise` directory, `mise` will delete them with the next reshim.

## Shims vs PATH {#shims-vs-path}

The following features are affected when shims are used **instead** of [PATH activation](#path-activation):

- [Env vars](/environments/) defined in mise are only available to mise tools
- Most [hooks](/hooks.html) won't trigger
- The unix `which` command points to the shim, obscuring the real executable

In general, using PATH (`mise activate`) instead of shims for _interactive_ situations is recommended.

The way `activate` works is every time the prompt is displayed, mise-en-place will determine what PATH and other
env vars should be and export them. This is why it doesn't work well for non-interactive situations like scripts. The prompt never gets displayed so you have to manually call `mise hook-env` to get mise to update
the env vars. (though there are exceptions, see [hook on `cd`](#hook-on-cd))

### Env vars and shims

A downside of shims is that the environment variables are only loaded when a shim is called. This means if you
set an [environment variable](/environments/) in `mise.toml`, it will only be used when a shim is called.

The following example only works under `mise activate`:

```sh
$ mise set NODE_ENV=production
$ echo $NODE_ENV
production
```

But this will work in either:

```sh
$ mise set NODE_ENV=production
$ node -p process.env.NODE_ENV
production
```

Also, [`mise x|exec`](/cli/exec.html) and [`mise r|run`](/cli/run.html) can be used to get the environment even if you don't need any mise tools:

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

The [hooks](/hooks.html) `cd`, `enter`, `exit`, and `watch_files` only trigger with `mise activate`. However `preinstall` and `postinstall` still work with shims because they don't require shell integration.

### `which`

`which` is a command that a lot of users find great value in. Using shims effectively "break" `which` and cause it to show the location of the shim. A workaround is to use `mise which` will show the actual location. Some users prefer the "cleanliness" of running `which node` and getting back a real path with a version number inside of it. e.g:

```sh
$ which node
~/.mise/installs/node/20/bin/node
```

### Performance

Truthfully, you're probably not going to notice a difference in performance when using shims vs. using `mise activate`.

- Since mise runs every time the prompt is displayed with `mise activate`, you'll pay a few ms cost
  every time the prompt is displayed. Regardless of whether you're actively using a mise tool, you'll
  pay that penalty every time you run any command. It does have some short-circuiting logic to make it faster
  if there are no changes, but it doesn't help much unless you have a very complex setup.
- shims have basically the same performance profile but run when the shim is called. This makes some situations
  better, and some worse.

If you are calling a shim from within a bash script like this:

```sh
for i in {1..500}; do
    node script.js
done
```

You'll pay the mise penalty every time you call it within the loop. However, if you did the same thing
but call a subprocess from within a shim (say, node creating a node subprocess), you will _not_ pay a new
penalty. This is because when a shim is called, mise sets up the environment with PATH for all tools and
those PATH entries will be before the shim directory.

In other words, which is better in terms of performance just depends on how you're calling mise. Really
though most users will not notice a few ms lag on their terminal caused by `mise activate`.

The only difference between these would be that using `hook-env` you will need to call
it again if you change directories but with shims that won't be necessary. The shims directory will be
removed by `mise activate` automatically so you won't need to worry about dealing with shims in your PATH.

## Neither shims nor PATH {#neither-shims-nor-path}

There are many ways to load the mise environment that don't require either, chiefly:
[`mise x|exec`](/cli/exec.html), [`mise r|run`](/cli/run.html) or [`mise en`](/cli/en.html).

These will both load all the tools and env vars before executing something. This might
be ideal because you don't need to modify your shell rc file at all and the environment is always loaded
explicitly. Some might find this is a "clean" way of working.

The obvious downside is that anytime one wants to use `mise` they need to prefix it with `mise exec|run`. Though, you can easily alias them to `mx|mr`.

- This is what one prefers if they like things to be precise over "easy".
- Or perhaps if you're just wanting to use mise on a single project because that's what your team uses and prefer
  not to use it to manage anything else on your system. Using a shell extension for that use-case
  would be overkill.

::: info This is the method Jeff uses

> Part of the reason for this is I often need to make sure I'm on my development version of mise. If you
> work on mise yourself I would recommend working in a similar way and disabling `mise activate` or shims
> while you are working on it.
>
> See [How I use mise](https://mise.jdx.dev/how-i-use-mise.html) for more information.

:::

## Hook on `cd` {#hook-on-cd}

For some shells (`bash`, `zsh`, `fish`, `xonsh`), `mise` hooks into the `cd` command, while in others, it only runs when the prompt is displayed. This relies on `chpwd` in `zsh`, `PROMPT_COMMAND` in `bash`, `fish_prompt` in `fish`, and `on_chdir` in `xonsh`.

The upside is that it doesn't run as frequently but since mise is written in Rust the cost for executing
mise is negligible (a few ms).

::: details Running several commands in a single line

If you run a set of commands in a single line like the following:

```sh
cd ~
cd ~/src/proj1 && node -v && cd ~/src/proj2 && node -v
```

If using `mise activate`, in shell without hook on cd, this will use the tools from `~`, not from `~/src/proj1` or `~/src/proj2` even after the directory changed.

This is because, in these shells `mise` runs just before your prompt gets displayed whereas in others, it hooks on `cd`. Note that shims _will_ always work with the inline example above.

:::

## Using mise in rc files

rc files like `.zshrc` are unusual. It's a script but also runs only for interactive sessions. If you need
to access tools provided by mise inside of an rc file you have 2 options:

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
