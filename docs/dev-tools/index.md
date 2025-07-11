# Dev Tools

> _Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm)
> or [pyenv](https://github.com/pyenv/pyenv) but for any language), it manages dev tools like node,
> python, cmake, terraform, and [hundreds more](/registry.html)._

`mise` is a tool that manages installations of programming language runtimes and other tools for local development. For example, it can be used to manage multiple versions of Node.js, Python, Ruby, Go, etc. on the same machine.

Once [activated](/getting-started.html#activate-mise), mise can automatically switch between different versions of tools based on the directory you're in.
This means that if you have a project that requires Node.js 18 and another that requires Node.js 22, mise will automatically switch between them as you move between the two projects. See tools available for mise with in the [registry](/registry).

To know which tool version to use, mise will typically look for a `mise.toml` file in the current directory and its parents. To get an idea of how tools are specified, here is an example of a [mise.toml](/configuration.html) file:

```toml [mise.toml]
[tools]
node = '22'
python = '3'
ruby = 'latest'
```

It's also compatible
with asdf `.tool-versions` files as well as [idiomatic version files](/configuration#idiomatic-version-files) like `.node-version` and
`.ruby-version`. See [configuration](/configuration) for more details.

::: info
mise is inspired by [asdf](https://asdf-vm.com) and can leverage asdf's
vast [plugin ecosystem](https://github.com/mise-plugins/registry)
under the hood. However, [it is _much_ faster than asdf and has a more friendly user experience](./comparison-to-asdf).
:::

## How it works

mise manages development tools through a sophisticated but user-friendly system that automatically handles tool installation, version management, and environment setup.

### Tool Resolution Flow

When you enter a directory or run a command, mise follows this process:

1. **Configuration Discovery**: mise walks up the directory tree looking for configuration files (`mise.toml`, `.tool-versions`, etc.) and merges them hierarchically
2. **Tool Resolution**: mise resolves version specifications (like `node@latest` or `python@3`) to specific versions using registries and version lists
3. **Backend Selection**: mise chooses the appropriate [backend](/dev-tools/backends/architecture) to handle each tool (core, asdf, aqua, etc.)
4. **Installation Check**: mise verifies if the required tool versions are installed, automatically installing missing ones
5. **Environment Setup**: mise configures your `PATH` and environment variables to use the resolved tool versions

### Environment Integration

mise provides several ways to integrate with your development environment:

**Automatic Activation**: With `mise activate`, mise hooks into your shell prompt and automatically updates your environment when you change directories:

```bash
eval "$(mise activate zsh)"  # In your ~/.zshrc
cd my-project               # Automatically loads mise.toml tools
```

**On-Demand Execution**: Use `mise exec` to run commands with mise's environment without permanent activation:

```bash
mise exec -- node my-script.js  # Runs with tools from mise.toml
```

**Shims**: mise can create lightweight wrapper scripts that automatically use the correct tool versions:

```bash
mise activate --shims  # Creates shims instead of modifying PATH
```

### Path Management

mise modifies your `PATH` environment variable to prioritize the correct tool versions:

```bash
# Before mise
echo $PATH
/usr/local/bin:/usr/bin:/bin

# After mise activation in a project with node@20
echo $PATH
/home/user/.local/share/mise/installs/node/20.11.0/bin:/usr/local/bin:/usr/bin:/bin
```

This ensures that when you run `node`, you get the version specified in your project configuration, not a system-wide installation.

### Configuration Hierarchy

mise supports nested configuration that cascades from broad to specific settings:

```bash
~/.config/mise/config.toml      # Global defaults
~/work/mise.toml                # Work-specific tools
~/work/project/mise.toml        # Project-specific overrides
~/work/project/.tool-versions   # Legacy asdf compatibility
```

Each level can override or extend the previous ones, giving you fine-grained control over tool versions across different contexts.

### Caching and Performance

mise uses intelligent caching to minimize overhead:

- **Version lists**: Cached daily to avoid repeated API calls
- **Installation artifacts**: Cached downloads to speed up reinstalls
- **Environment resolution**: Cached environment setups for faster shell prompts
- **Plugin metadata**: Cached plugin information for quicker operations

This ensures that mise adds minimal latency to your daily development workflow.

::: info
After activating, mise will update env vars like PATH whenever the directory is changed or the prompt is _displayed_.
See the [FAQ](/faq#what-does-mise-activate-do).
:::

After activating, every time your prompt displays it will call `mise hook-env` to fetch new
environment variables.
This should be very fast. It exits early if the directory wasn't changed or
`mise.toml`/`.tool-versions` files haven't been modified.

`mise` modifies `PATH` ahead of time so the runtimes are called directly. This means that calling a tool has zero overhead and commands like `which node` returns the real path to the binary.
Other tools like asdf only support shim files to dynamically locate runtimes when they're called which adds a small delay and can cause issues with some commands. See [shims](/dev-tools/shims) for more information.

## Common commands

Here are some of the most important commands when it comes to working with dev tools. Click the
header
for each command to go to its reference documentation page to see all available flags/options and
more
examples.

### [`mise use`](/cli/use)

For some users, `mise use` might be the only command you need to learn. It will do the following:

- Install the tool's plugin if needed
- Install the specified version
- Set the version as active (i.e. update the `PATH`)
- Update the current configuration file (`mise.toml` or `.tool-versions`)

```shell
> cd my-project
> mise use node@22
# download node, verify signature...
mise node@22.12.0 ✓ installed
mise ~/my-project/mise.toml tools: node@22.12.0 # mise.toml created/updated

> which node
~/.local/share/installs/node/22.12.0/bin/node
```

`mise use node@22` will install the latest version of node-22 and create/update the
`mise.toml`
config file in the local directory. Anytime you're in that directory, that version of `node` will be
used.

`mise use -g node@22` will do the same but update the [global config](/configuration.html#global-config-config-mise-config-toml) (~/.config/mise/config.toml) so
unless there is a config file in the local directory hierarchy, node-22 will be the default version
for
the user.

### [`mise install`](/cli/install)

`mise install` will install but not activate tools—meaning it will download/build/compile the tool
into `~/.local/share/mise/installs` but you won't be able to use it without "setting" the version
in a `.mise-toml` or `.tool-versions` file.

::: tip
If you're coming from `asdf`, there is no need to also run `mise plugin add` to first install
the plugin, that will be done automatically if needed. Of course, you can manually install plugins
if you wish or you want to use a plugin not in the default registry.
:::

There are many ways it can be used:

- `mise install node@20.0.0` - install a specific version
- `mise install node@20` - install the latest version matching this prefix
- `mise install node` - install whatever version of node currently specified in `mise.toml` (or other
  config files)
- `mise install` - install all plugins and tools specified in the config files

### [`mise exec`|`mise x`](/cli/exec)

`mise x` can be used for one-off commands using specific tools. e.g.: if you want to run a script
with python3.12:

```sh
mise x python@3.12 -- ./myscript.py
```

Python will be installed if it is not already. `mise x` will read local/global
`.mise-toml`/`.tool-versions` files
as well, so if you don't want to use `mise activate` or shims you can use mise by just prefixing
commands with
`mise x --`:

```sh
$ mise use node@20
$ mise x -- node -v
20.x.x
```

::: tip
If you use this a lot, an alias can be helpful:

```sh
alias mx="mise x --"
```

:::

Similarly, `mise run` can be used to [execute tasks](/tasks/) which will also activate the mise
environment with all of your tools.

## Tool Options

mise plugins may accept configuration in the form of tool options specified in `mise.toml`:

```toml
[tools]
# send arbitrary options to the plugin, passed as:
# MISE_TOOL_OPTS__FOO=bar
mytool = { version = '3.10', foo = 'bar' }
```

All tools can accept a `postinstall` option which is a shell command to run after the tool is installed:

```toml
[tools]
node = { version = '20', postinstall = 'corepack enable' }
```

It's yet not possible to specify this via the CLI in `mise use`. As a workaround, you can use [mise config set](/cli/config/set.html):

```shell
mise config set tools.node.version 20
mise config set tools.node.postinstall 'corepack enable'
mise install
```

### `install_env`

`install_env` is a special option that can be used to set environment variables during tool installation:

```toml
[tools]
teleport-ent = { version = "11.3.11", install_env = { TELEPORT_ENT_ARCH = "amd64" } }
```
