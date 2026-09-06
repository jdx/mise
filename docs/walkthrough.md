# Walkthrough

Use this guide to add mise to an existing project and maintain its configuration:
select tools, share defaults, upgrade versions, and run everyday commands.
Complete [getting started](/getting-started.html) first if you haven't installed mise.

The examples use `mise exec` and `mise run`, so shell activation is optional.
With [activation](/getting-started.html#activate-mise), you can also run the
selected tools directly at your prompt.

## Installing Dev Tools

The main command for working with tools in mise is [`mise use`](/cli/use). It does two things:

- Installs the tool (if not already installed)
- Saves its version request in the project configuration

`mise install node@24` downloads a tool without selecting it for your project.
Use `mise use` to save that selection, or `mise exec node@24 -- node --version`
to use it for one command.

Run these from the existing project's root (or create a scratch directory first):

```bash
mise use node@24
mise exec -- node --version
# v24.x.x
```

You'll also notice that you now have a `mise.toml` file with the following content:

```mise-toml [mise.toml]
[tools]
node = "24"
```

- If this file is in the root of a project, `node` will be installed whenever someone runs [`mise install`](/cli/install).
- `mise install` installs the configured tools after you clone a project. Use `mise upgrade` to update them.

## `mise.toml` Configuration

You can create a `mise.toml` file manually or with the CLI.

> [!TIP]
> Use `mise edit` to open an interactive editor for your configuration. It provides a TUI where you can navigate sections, add tools from the registry with fuzzy search, and configure settings with schema-aware autocompletion.

Use [`mise.toml`](/configuration#mise-toml) to share your tool configurations with others. This file should be committed to version control and contains the common toolset needed for your project.

For tools or settings you want to keep private, use [`mise.local.toml`](/configuration#mise-toml). This file should be added to `.gitignore` and is ideal for personal preferences or configuration.

`mise` supports nested configuration files that cascade from broad to specific settings:

1. `~/.config/mise/config.toml` - Global settings for all projects
2. `~/work/mise.toml` - Work-specific settings
3. `~/work/project/mise.toml` - Project-specific settings
4. `~/work/project/mise.local.toml` - Project-specific settings that should not be shared

`mise` combines the config files from all parent directories to determine the set of tools—with lower levels of the hierarchy overriding higher ones.

:::tip
Use [`mise config ls`](/cli/config/ls) to see the configuration files currently used by `mise`.
:::

Choose version precision to match the project. A request such as `node@24` allows
releases within that series. Use `mise use --pin` to save an exact version, or a
[lockfile](/dev-tools/mise-lock.html) to share resolved versions while keeping
broader requests in `mise.toml`.

If you leave out the version, mise defaults to `node@latest`.

## Dev Tool Backends

Tools are installed with a variety of backends like `aqua`, `github`, or `gitlab`. See [registry](/registry.html) for
the full list of shorthands like `node` you can use.

You can also use other backends like `npm` or `cargo`,
for command-line packages from their respective registries. Declare the
required runtime or compiler too:

```bash
mise use node@24 'npm:@antfu/ni'
mise use rust@stable cargo:starship
```

## Upgrading Dev Tools

Upgrade tool versions with [`mise upgrade`](/cli/upgrade). By default, it respects
the version prefix in `mise.toml`. If a [lockfile](/configuration/settings#lockfile) exists,
mise updates `mise.lock` to the latest version of the tool matching the prefix from `mise.toml`.

So if you have `node = "24"` in `mise.toml`, then `mise upgrade node` will upgrade to the latest version of `node 24`.

To update the version in `mise.toml` to something newer, use `mise upgrade --bump node`.
It keeps the same specificity as the current version: if you have `node = "24"`
and `mise upgrade --bump node` updates to `node@26`, it will set `node = "26"` in `mise.toml`.

_See [Dev Tools](/dev-tools/) for more information on working with tools._

## Setting Environment Variables

mise can also set environment variables for your project. You can set them
with the CLI:

```bash
mise set MY_VAR=123
mise exec -- node -p process.env.MY_VAR
# 123
```

Or by directly modifying `mise.toml`:

```toml
[env]
MY_VAR = "123"
```

Some examples of where this is useful:

- Setting `NODE_ENV` for a Node.js project
- Setting `DATABASE_URL` for a database connection
- Setting `RUST_TEST_THREADS=1` to run cargo tests in series

Keep secrets out of committed configuration. Use an ignored local file or a
[secret provider](/environments/secrets/). `mise.local.toml` is still plaintext;
its name does not encrypt its contents or automatically keep it out of Git.

You can also modify `PATH` with `mise.toml`.
This example makes CLIs installed with `npm` available:

```toml
[env]
_.path = "./node_modules/.bin"
```

This adds `./node_modules/.bin` to the PATH for the project. Here "." refers to the directory
containing the `mise.toml` file, so the entry still works if you enter a subdirectory.

_See [Environments](/environments/) for more information on working with environment variables._

## Tasks

Tasks are defined in a project to execute commands.

If the project's `package.json` already defines `build` and `test` scripts, add
these task wrappers. Install the project's npm dependencies before running them:

```sh
mise exec -- npm ci
```

This assumes the project has a committed `package-lock.json`. Use its chosen
package manager and lockfile when it does not. Then add to `mise.toml`:

```mise-toml [mise.toml]
[tasks]
build = "npm run build"
test = "npm test"
```

Alternatively, define `build` as a file task in `mise-tasks/build`. Choose one
definition for a task name:

```bash [mise-tasks/build]
#!/bin/bash
npm run build
```

On Unix, make file tasks executable with `chmod +x mise-tasks/build`.
Tasks are executed with [`mise run`](/cli/run):

```bash
mise run build
mise run test
```

:::tip
`mise run` sets up the "mise environment" (tools and environment variables) before running the task.
So if you'd rather not activate mise in your shell, you can use `mise run` to run tasks with the
tools on PATH and the environment variables from `mise.toml` set.
:::

`mise` is paired with [usage](https://usage.jdx.dev), which provides lots of features for documenting and running tasks.

Here is an example of a task with usage spec:

```bash [mise-tasks/greet]
#!/usr/bin/env bash
set -e

#MISE description="Greet a user with a message"
#USAGE flag "-g --greeting <greeting>" help="The greeting word to use" default="hello" {
#USAGE   choices "hi" "hello" "hey"
#USAGE }
#USAGE flag "-u --user <user>" help="The user to greet" default="world"
#USAGE flag "--dir <dir>" help="The directory to greet from" default="."
#USAGE complete "dir" run="find . -maxdepth 1 -type d"
#USAGE arg "<message>" help="Greeting message"

echo "${usage_greeting?}, ${usage_user?}! Your message is: ${usage_message?}"
```

Save the script as `mise-tasks/greet` and make it executable on Unix:

```sh
chmod +x mise-tasks/greet
```

Then run it:

```shell
mise run greet --user jdx -g "hey" "How are you?"
```

- All options are passed as environment variables prefixed with `usage_`, like `usage_user`.
- Help is available with `mise run greet --help`, which shows the options defined in the task.
- Completions are available like you'd expect, so typing `mise run greet --greeting <tab>` will show `hi`, `hello`, and `hey`
  as options.
- [Custom completion](https://usage.jdx.dev/spec/reference/complete) can be provided by a CLI. `mise run greet --dir <tab>` will execute `find . -maxdepth 1 -type d` to provide completions.

To get the autocompletion working, set up [mise autocompletions](/installing-mise.html#autocompletion).

_See [Tasks](/tasks/) for more information on working with tasks._

## Common Commands

| Task                            | Command                                                                                |
| ------------------------------- | -------------------------------------------------------------------------------------- |
| Show active configuration files | [`mise config ls`](/cli/config/ls.html)                                                |
| Inspect selected tool versions  | [`mise ls --current`](/cli/ls.html)                                                    |
| Find available releases         | [`mise ls-remote TOOL`](/cli/ls-remote.html)                                           |
| See tools with updates          | [`mise outdated`](/cli/outdated.html)                                                  |
| List project tasks              | [`mise tasks ls`](/cli/tasks/ls.html)                                                  |
| Diagnose environment problems   | [`mise doctor`](/cli/doctor.html)                                                      |
| Update mise itself              | [`mise self-update`](/cli/self-update.html), or the package manager used to install it |

See the [CLI reference](/cli/) for all commands and options.

## Further reading {#final-thoughts}

Use the feature guides for options beyond this workflow:

- [Dev Tools](/dev-tools/) – A deeper overview of working with dev tools
- [Environments](/environments/) – A deeper overview of working with environment variables
- [Tasks](/tasks/) – A deeper overview of working with tasks
- [Configuration](/configuration) – More information on `mise.toml` files
- [Settings](/configuration/settings) – All the configuration settings available in mise
- [Backends](/dev-tools/backends/) – An index of all the backends available in mise
- [Registry](/registry) – Every "shorthand" available for tools in mise like `node`, `terraform`, or `watchexec` which point to `core:node`, `aqua:hashicorp/terraform`, and `aqua:watchexec/watchexec` respectively
- [CLI](/cli/) – The full list of commands available in mise
