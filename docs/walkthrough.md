# Walkthrough

Once you've completed the [Getting Started](/getting-started) guide, you're ready to start using mise.
This document offers a quick overview on some initial things you may want to try out.

## Installing Dev Tools

The main command for working with tools in mise is [`mise use|u`](/cli/use). This does 2 main things:

- Installs tools (if not already installed)
- Adds the tool to the `mise.toml` configuration file—in mise I say the tool is "active" if it's in `mise.toml`

:::warning
Both of these are required to use a tool. If you simply install a tool via `mise install`, it won't be available in your shell.
It must also be added to `mise.toml`—which is why I promote using `mise use` since it does both.
:::

You use it like so (note that `mise` must be [activated](/getting-started.html#_2-activate-mise) for this to work):

```bash
mkdir example-project && cd example-project
mise use node@22
node -v
# v22.0.0
```

And you'll also note that you now have a `mise.toml` file with the following content:

```shell
cat mise.toml
```

```toml
[tools]
node = "22"
```

- If this file is in the root of a project, `node` will be installed whenever someone runs [`mise install|i`](/cli/install).
- This is the command you want to run when you first clone a project or when you want to update installed tools.

## `mise.toml` Configuration

Use [`mise.toml`](/configuration#mise-toml) to share your tool configurations with others. This file should be committed to version control and contains the common toolset needed for your project.

For tools or settings you want to keep private, use [`mise.local.toml`](/configuration#mise-toml). This file should be added to `.gitignore` and is perfect for personal preferences or configurations.

`mise` supports nested configuration files that cascade from broad to specific settings:

1. `~/.config/mise/config.toml` - Global settings for all projects
2. `~/work/mise.toml` - Work-specific settings
3. `~/work/project/mise.toml` - Project-specific settings
4. `~/work/project/mise.local.toml` - Project-specific settings that should not be shared

`mise` will use all the parent directories together to determine the set of tools—overriding configuration as it goes lower in the hierarchy.

:::tip
Use [`mise config ls`](/cli/config/ls) to see the configuration files currently used by `mise`.
:::

In general, it's preferred to use loose versions like this in `mise` so that other people working
on a project don't have to worry about the exact version of a tool you're using. If you'd like to
pin the version to enforce a specific version, use `mise use --pin` or the [`lockfile`](/configuration/settings#lockfile) setting.

If you leave out the version, then mise will default to `node@latest`.

## Dev Tool Backends

Tools are installed with a variety of backends like `asdf`, `ubi`, or `vfox`. See [registry](/registry.html) for
the full list of shorthands like `node` you can use.

You can also use other backends like `npm` or `cargo`
which can install any package from their respective registries:

```bash
mise use npm:@antfu/ni
mise use cargo:starship
```

## Upgrading Dev Tools

Upgrading tool versions can be done with [`mise upgrade|up`](/cli/upgrade). By default, it will respect
the version prefix in `mise.toml`. If a [lockfile](/configuration/settings#lockfile) exists,
mise will update `mise.lock` to the latest version of the tool with the prefix from `mise.toml`.

So if you have `node = "22"` in `mise.toml`, then `mise upgrade node` will upgrade to the latest version of `node 22`.

If you'd like to upgrade to the latest version of node, you can use `mise upgrade --bump node`. It will set the version
at the same specificity as the current version, so if you have `node = "22"`, but use `mise upgrade --bump node` to update to
`node@24`, then it will set `node = "24"` in `mise.toml`.

_See [Dev Tools](/dev-tools/) for more information on working with tools._

## Setting Environment Variables

mise can also be used to set environment variables for your project. You can set environment variables
with the CLI:

```bash
mise set MY_VAR=123
echo $MY_VAR
# 123
```

Or by directly modifying `mise.toml`:

```toml
[env]
MY_VAR = "123"
```

Some examples on where this can be used:

- Setting `NODE_ENV` for a Node.js project
- Setting `DATABASE_URL` for a database connection
- Setting `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` for AWS
- Setting `RUST_TEST_THREADS=1` to run cargo tests in series

You can also modify `PATH` with `mise.toml`.
This example makes CLIs installed with `npm` available:

```toml
[env]
_.path = "./node_modules/.bin"
```

This will add `./node_modules/.bin` to the PATH for the project—with "." here referring to the directory
the `mise.toml` file is in so if you enter a subdirectory, it will still work.

_See [Environments](/environments) for more information on working with environment variables._

## Tasks

Tasks are defined in a project to execute commands. They are defined in `mise.toml`:

```toml
[tasks]
build = "npm run build"
test = "npm test"
```

Or in a `mise-tasks` directory as a standalone file, such as `mise-tasks/build`:

```bash
#!/bin/bash
npm run build
```

Tasks are executed with [`mise run|r`](/cli/run):

```bash
mise run build
mise run test
```

:::tip
`mise run` setups up the "mise environment" before running the task (tools and environment variables).
So if you'd rather not activate mise in your shell, you can use `mise run` to run tasks, and it will
have the tools in PATH and the environment variables from `mise.toml` set.
:::

mise is paired with [usage](https://usage.jdx.dev) which provides lots of features for documenting and running tasks.

Here is an example of a task with usage spec:

```bash
#!/bin/bash
#USAGE flag "-f --force" "Force the greeting to be displayed"
#USAGE flag "-u --user <user>" "The user to greet"
#USAGE flag "-p --port <port>" default=8080 "The port to listen on"
#USAGE flag "-c --color <color>" "The color to use" {
#USAGE     choices "red" "green" "blue"
#USAGE }
#USAGE arg "message" "The message to greet the user with"
#USAGE complete "user" run="mycli users"

echo "Hello, $usage_user! Your message is: $usage_message"
```

The options will all be passed as environment variables prefixed with `usage_` like `usage_user`.
Help is available with `mise run my-task --help` and will show the options defined in the task.
Completions are available like you'd expect, so typing `mise run my-task --color <tab>` will show "red", "green", and "blue"
as options. `mise run my-task --user <tab>` will execute `mycli users` and use the output as completions.

No extra setup is required for completions so long as [mise completions](/cli/completion) are otherwise set up.

_See [Tasks](/tasks/) for more information on working with tasks._

## Final Thoughts

Dev tools, env vars, and tasks work together to make managing your development environment easier—especially
when working with others. The goal is to have a consistent UX to interface with projects regardless of the
programming languages or tools used on it.

For further reading:

- [Dev Tools](/dev-tools/) – A deeper overview of working with dev tools
- [Environments](/environments) – A deeper overview of working with environment variables
- [Tasks](/tasks/) – A deeper overview of working with tasks
- [Configuration](/configuration) – More information on `mise.toml` files
- [Settings](/configuration/settings) – All the configuration settings available in mise
- [Backends](/dev-tools/backends/) – An index of all the backends available in mise
- [Registry](/registry) – Every "shorthand" available for tools in mise like `node`, `terraform`, or `watchexec` which point to `core:node`, `asdf:asdf-community/asdf-hashicorp`, and `ubi:watchexec/watchexec` respectively
- [CLI](/cli/) – The full list of commands available in mise

Since there are a lot of commands available in mise, here are what I consider the most important:

- [`mise completion`](/cli/completion) – Set up completions for your shell.
- [`mise config|cfg`](/cli/config) – A bunch of commands for working with `mise.toml` files via the CLI.
- [`mise exec|x`](/cli/exec) – Execute a command in the mise environment without activating mise.
- [`mise generate|g`](/cli/generate) – Generates things like git hooks, task documentation, GitHub actions, and more for your project.
- [`mise install|i`](/cli/install) – Install tools.
- [`mise link`](/cli/link) – Symlink a tool installed by some other means into the mise.
- [`mise ls-remote`](/cli/ls-remote) – List all available versions of a tool.
- [`mise ls`](/cli/ls) – Lists information about installed/active tools.
- [`mise outdated`](/cli/outdated) – Informs you of any tools with newer versions available.
- [`mise plugin`](/cli/plugins) – Plugins can extend mise with new functionality like extra tools or environment variable management. Commonly, these are simply asdf/vfox plugins.
- [`mise run|r`](/cli/run) – Run a task defined in `mise.toml` or `mise-tasks`.
- [`mise self-update`](/cli/self-update) – Update mise to the latest version. Don't use this if you installed mise via a package manager.
- [`mise settings`](/cli/settings) – CLI access to get/set configuration settings.
- [`mise uninstall|rm`](/cli/uninstall) – Uninstall a tool.
- [`mise upgrade|up`](/cli/upgrade) – Upgrade tool versions.
- [`mise use|u`](/cli/use) – Install and activate tools.
- [`mise watch|w`](/cli/watch) – Watch for changes in a project and run tasks when they occur.
