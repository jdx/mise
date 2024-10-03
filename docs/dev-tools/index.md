# Dev Tools

_Like [asdf](https://asdf-vm.com) (or [nvm](https://github.com/nvm-sh/nvm)
or [pyenv](https://github.com/pyenv/pyenv) but for any language) it manages dev tools like node,
python, cmake, terraform, and [hundreds more](/plugins)._

::: tip
New developer? Try reading the [Beginner's Guide](https://dev.to/jdxcode/beginners-guide-to-rtx-ac4)
for a gentler introduction.
:::

mise is a tool for managing programming language and tool versions. For example, use this to install
a particular version of Node.js and ruby for a project. Using `mise activate`, you can have your
shell automatically switch to the correct node and ruby versions when you `cd` into the project's
directory. Other projects on your machine can use a different set of versions.

mise is inspired by [asdf](https://asdf-vm.com) and uses asdf's
vast [plugin ecosystem](https://github.com/mise-plugins/registry)
under the hood. However, it is _much_ faster than asdf and has a more friendly user experience.
For more on how mise compares to asdf, [see below](./comparison-to-asdf). See plugins available for
mise with
[`mise plugins ls-remote`](/cli/plugins/ls-remote).

mise can be configured in many ways. The most typical is by `.mise.toml`, but it's also compatible
with asdf `.tool-versions` files. It can also use idiomatic version files like `.node-version` and
`.ruby-version`. See [Configuration](/configuration) for more.

* Like [direnv](https://github.com/direnv/direnv) it
  manages [environment variables](/configuration#env---arbitrary-environment-variables) for
  different project directories.
* Like [make](https://www.gnu.org/software/make/manual/make.html) it manages [tasks](/tasks/) used
  to build and test projects.

## How it works

mise hooks into your shell (with `mise activate zsh`) and sets the `PATH`
environment variable to point your shell to the correct runtime binaries. When you `cd` into a
directory containing a `.tool-versions`/`.mise.toml` file, mise will automatically set the
appropriate tool versions in `PATH`.

::: info
mise does not modify "cd". It actually runs every time the prompt is _displayed_.
See the [FAQ](/faq#what-does-mise-activate-do).
:::

After activating, every time your prompt displays it will call `mise hook-env` to fetch new
environment variables.
This should be very fast. It exits early if the directory wasn't changed or `.tool-versions`/
`.mise.toml` files haven't been modified.

Unlike asdf which uses shim files to dynamically locate runtimes when they're called, mise modifies
`PATH` ahead of time so the runtimes are called directly. This is not only faster since it avoids
any overhead, but it also makes it so commands like `which node` work as expected. This also
means there isn't any need to run `asdf reshim` after installing new runtime binaries.

You should note that mise does not directly install these tools.
Instead, it leverages plugins to install runtimes.
See [plugins](/plugins) below.

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
- Set the version as active (it's in PATH)

`mise use node@20` will install the latest version of node-20 and create/update the
.tool-versions/.mise.toml
config file in the local directory. Anytime you're in that directory, that version of node will be
used.

`mise use -g node@20` will do the same but update the global config (~/.config/mise/config.toml) so
unless there is a config file in the local directory hierarchy, node-20 will be the default version
for
the user.

### [`mise install`](/cli/install)

`mise install` will install but not activate tools—meaning it will download/build/compile the tool
into `~/.local/share/mise/installs` but you won't be able to use it without "setting" the version
in a `.tool-versions` or `.mise-toml` file.

::: tip
If you're coming from asdf, there is no need to also run `mise plugin add` to first install
the plugin, that will be done automatically if needed. Of course, you can manually install plugins
if you wish or you want to use a plugin not in the default registry.
:::

There are many ways it can be used:

* `mise install node@20.0.0` - install a specific version
* `mise install node@20` - install the latest version matching this prefix
* `mise install node` - install whatever version of node currently specified in
  .tool-versions/.mise.toml
* `mise install` - install all plugins and tools

### `mise local|global` <Badge type="danger" text="not recommended" />

`mise local` and `mise global` are command which only modify the `.tool-versions` or `.mise.toml`
files.
These are hidden from the CLI help and remain for asdf-compatibility. The recommended approach is
to use `mise use` instead because that will do the same thing but also install the tool if it does
not already exists.

### [`mise exec`|`mise x`](/cli/exec)

`mise x` can be used for one-off commands using specific tools. e.g.: if you want to run a script
with python3.12:

```sh
mise x python@3.12 -- ./myscript.py
```

Python will be installed if it is not already. `mise x` will read local/global `.tool-versions`/
`.mise-toml` files
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

Unfortunately at the time of this writing, it's not possible to specify this via the CLI in
`mise use` or other commands though. See <https://github.com/jdx/mise/issues/2309>
