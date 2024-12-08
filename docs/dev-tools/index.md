# Dev Tools

> _Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm)
or [pyenv](https://github.com/pyenv/pyenv) but for any language), it manages dev tools like node,
python, cmake, terraform, and [hundreds more](/registry.html)._

::: tip
New developer? Try reading the [Beginner's Guide](https://dev.to/jdxcode/beginners-guide-to-rtx-ac4)
for a gentler introduction.
:::

mise is a tool that manages installations of programming language runtimes and other tools for local development. For example, it can be used to manage multiple versions of Node.js, Python, Ruby, Go, etc. on the same machine.

Once [activated](/getting-started.html#_2-activate-mise), mise will automatically switch between different versions of tools based on the directory you're in.
This means that if you have a project that requires Node.js 18 and another that requires Node.js 22, mise will automatically switch between them as you move between the two projects. See tools available for mise with in the [registry](/registry).

To know which tool version to use, mise will typically look for a `mise.toml` file in the current directory and its parents. To get an idea of how tools are specified, here is an example of a [mise.toml](/configuration.html) file:
```toml
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

mise hooks into your shell (with `mise activate zsh`) and sets the `PATH`
environment variable to point your shell to the correct runtime binaries. When you `cd` into a
directory containing a `mise.toml`/`.tool-versions` file, mise will automatically set the
appropriate tool versions in `PATH`.

::: info
mise does not modify `cd`. It actually runs every time the prompt is _displayed_.
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
