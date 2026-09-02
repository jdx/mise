# Dev Tools

> _Install and switch between dev tools like node, python, cmake, terraform,
> and [hundreds more](/registry.html), all from the same project config._

`mise` manages installations of programming language runtimes and other tools for local development. For example, it can manage multiple versions of Node.js, Python, Ruby, and Go on the same machine.

Once [activated](/getting-started.html#activate-mise), mise can automatically switch between different versions of tools based on the directory you're in.
This means that if you have a project that requires Node.js 18 and another that requires Node.js 22, mise automatically switches between them as you move between the two projects. See the tools available for mise in the [registry](/registry).

To determine which tool version to use, mise typically looks for a `mise.toml` file in the current directory and its parents. Here is an example of a [mise.toml](/configuration.html) file showing how tools are specified:

```toml [mise.toml]
[tools]
node = '24'
python = '3'
ruby = 'latest'
```

mise is also compatible
with asdf `.tool-versions` files and with [idiomatic version files](/configuration#idiomatic-version-files) like `.node-version` and
`.ruby-version`. See [configuration](/configuration) for more details.

When specifying tool versions and tool options, you can also refer to environment variables or
[`vars`](/configuration/vars) defined in your config hierarchy, including values
produced by directives like `_.source`, `_.file`, or env modules. These are resolved before tool
version and option templates are rendered.

::: info
mise is compatible with asdf `.tool-versions` files and can still use asdf
plugins when needed. If you're migrating from asdf, see the
[comparison guide](./comparison-to-asdf).
:::

## How it works

mise manages development tools through a sophisticated but user-friendly system that automatically handles tool installation, version management, and environment setup.

### Tool Resolution Flow

When you enter a directory or run a command, mise follows this process:

1. **Configuration Discovery**: mise walks up the directory tree looking for configuration files (`mise.toml`, `.tool-versions`, etc.) and merges them hierarchically
2. **Tool Resolution**: mise resolves version specifications (like `node@latest` or `python@3`) to specific versions using registries and version lists
3. **Backend Selection**: mise chooses the appropriate [backend](/dev-tools/backend_architecture) to handle each tool (core, asdf, aqua, etc.)
4. **Installation Check**: mise checks whether the required tool versions are installed and automatically installs missing ones
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

## Tool Options

Tool options let you customize how tools are installed and configured. They support nested configuration for better organization, which is particularly useful for platform-specific settings.

### Table Format (Recommended)

The cleanest way to specify nested options is with TOML tables:

```toml
[tools."http:my-tool"]
version = "1.0.0"

[tools."http:my-tool".platforms]
macos-x64 = {
  url = "https://example.com/my-tool-macos-x64.tar.gz",
  checksum = "sha256:abc123",
}
linux-x64 = {
  url = "https://example.com/my-tool-linux-x64.tar.gz",
  checksum = "sha256:def456",
}
```

### Dotted Notation

You can also use dotted notation for simpler nested configurations:

```toml
[tools."http:my-tool"]
version = "1.0.0"
platforms.macos-x64.url = "https://example.com/my-tool-macos-x64.tar.gz"
platforms.linux-x64.url = "https://example.com/my-tool-linux-x64.tar.gz"
simple_option = "value"
```

### Generic Nested Support

Any backend can use nested options for organizing complex configurations:

```toml
[tools."custom:my-backend"]
version = "1.0.0"

[tools."custom:my-backend".database]
host = "localhost"
port = 5432

[tools."custom:my-backend".cache.redis]
host = "redis.example.com"
port = 6379
```

Internally, nested options are flattened to dot notation (e.g., `platforms.macos-x64.url`, `database.host`, `cache.redis.port`) for backend access.

### Version ordering

Backends normally preserve the order returned by their version source. Aqua,
GitHub, GitLab, Forgejo, and HTTP tools can opt into semantic version precedence
when an upstream publishes backports after newer release lines:

```toml
[tools]
"github:owner/tool" = { version = "latest", version_order = "semver" }
```

For `latest`, an authoritative result from the backend still wins—for example,
the release marked **Latest** on GitHub or Forgejo. If that release does not
match the requested package, or the backend has no authoritative latest result,
mise falls back to the version list and applies `version_order` there. This is
important for repositories containing multiple products: their repository-wide
Latest release may not contain an asset for every package.

With `version_order = "semver"`, mise orders valid semantic versions by
precedence in `mise ls-remote` output and when resolving that list or a version
prefix. Opaque versions retain their source order before semantic versions, so
exact requests such as `nightly` continue to work. Build metadata does not affect
precedence. Registry entries may set this option for tools known to follow
semantic versioning; users can set `version_order = "source"` to restore the
backend's default ordering.

### Tool postinstall commands

Run a command immediately after a tool finishes installing by adding a `postinstall` field to that tool's configuration. This is separate from `[hooks].postinstall` and applies only when that specific tool is installed.

```toml
[tools]
node = { version = "22", postinstall = "corepack enable" }
```

Behavior:

- The command runs once the install completes successfully for that tool/version.
- The tool's bin path is on PATH during the command, so you can invoke the installed tool directly.
- Environment variables include `MISE_TOOL_INSTALL_PATH` pointing to the tool's install directory and any variables from that tool's `install_env` option.
- If the install fails, the `postinstall` command is not run.

## OS-Specific Tools

You can restrict tools to specific operating systems using the `os` field:

```toml
[tools]
# Only install on Linux and macOS
ripgrep = { version = "latest", os = ["linux", "macos"] }

# Only install on Windows
"npm:windows-terminal" = { version = "latest", os = ["windows"] }

# Works with other options
"cargo:usage-cli" = {
    version = "latest",
    os = ["linux", "macos"],
    locked = false
}
```

The `os` field accepts an array of operating system identifiers:

- `"linux"` - All Linux distributions
- `"macos"` - macOS (Darwin). `"darwin"` is also accepted as an alias.
- `"windows"` - Windows. `"win"` is also accepted as an alias.

### OS/Architecture Combinations

You can also restrict tools to specific OS and architecture combinations using the `os/arch` syntax:

```toml
[tools]
# Only install on macOS ARM64 and all Linux (skips macOS x86_64)
hk = { version = "latest", os = ["linux", "macos/arm64"] }

# Only install on Linux x86_64
mytool = { version = "latest", os = ["linux/x64"] }
```

Supported architecture identifiers:

- `"arm64"` (or `"aarch64"`)
- `"x64"` (or `"x86_64"` or `"amd64"`)

When an entry contains `/`, both the OS and architecture must match. When an entry is just an OS name, it matches any architecture on that OS.

If a tool specifies an `os` restriction and the current operating system is not in the list, mise skips installing and using that tool.

## Tool Dependencies

You can declare explicit installation dependencies between tools using the `depends` field. This ensures that one tool is fully installed before another begins installing.

```toml
[tools]
python = "3.12.11"
"pipx:ruff" = { version = "latest", depends = ["python"] }
```

In this example, `pipx:ruff` waits for `python` to finish installing before it starts.

The `depends` field accepts either a single string or an array of strings:

```toml
[tools]
# Single dependency
"pipx:ruff" = { version = "latest", depends = "python" }

# Multiple dependencies
"pipx:ruff" = { version = "latest", depends = ["python", "pipx"] }
```

User-specified `[tools].depends` adds ordering constraints and makes matching tools available to install hooks. Backend declarations such as vfox `PLUGIN.depends` are combined with these user declarations in the same install dependency context.

Dependency declarations do not add tools to the configuration or install them automatically. When a matching tool is configured, its selected version must resolve and already be installed (or finish successfully earlier in the same install batch). A declaration with no matching configured tool may still be satisfied by an executable on the existing system or configuration `PATH`.

### vfox plugin hook dependencies

vfox plugin authors should declare requirements intrinsic to the plugin on the `PLUGIN` table in `metadata.lua`:

```lua
PLUGIN = {
    name = "example",
    version = "1.0.0",
    depends = { "go" },
}
```

Use tool names as they would appear in `mise.toml`. Users can supplement plugin declarations with `[tools].depends`; both forms affect install ordering, the `PATH` visible to `os.execute` and `cmd.exec`, and `tools = true` environment values. They do not affect `io.popen`. See [Tool plugin development](/tool-plugin-development#_2-metadata-lua).

## Caching and Performance

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

After activating, every time your prompt is displayed, the shell calls `mise hook-env` to fetch new
environment variables.
This should be very fast: it exits early if the directory hasn't changed and no
`mise.toml`/`.tool-versions` files have been modified.

`mise` modifies `PATH` ahead of time so the tools are called directly. This means that calling a tool has zero overhead, and commands like `which node` return the real path to the binary.
Other tools like asdf only support shim files, which dynamically locate tools when they're called; this adds a small delay and can cause issues with some commands. See [shims](/dev-tools/shims) for more information.

## Common commands

Here are some of the most important commands for working with dev tools. Click a command's
header to open its reference page, which lists all available flags/options and more examples.

### [`mise use`](/cli/use)

For some users, `mise use` might be the only command they need to learn. It does the following:

- Install the tool's plugin if needed
- Install the specified version
- Set the version as active (i.e. update the `PATH`)
- Update the current configuration file (`mise.toml` or `.tool-versions`)

```shell
> cd my-project
> mise use node@26
# download node, verify signature...
mise node@26.x.x ✓ installed
mise ~/my-project/mise.toml tools: node@26.x.x # mise.toml created/updated

> which node
~/.local/share/mise/installs/node/26/bin/node
```

`mise use node@26` installs the latest version of node 26 and creates/updates the
`mise.toml`
config file in the current directory. The resulting file looks like this:

```toml [mise.toml]
[tools]
node = "26"
```

Whenever you're in that directory, that version of `node` is used.

`mise use -g node@26` does the same but updates the [global config](/configuration.html#global-config-config-mise-config-toml) (`~/.config/mise/config.toml`), so
node 26 is the default version for the user unless a config file in the local directory hierarchy
overrides it.

You can also edit `mise.toml` directly instead of using `mise use` — the effect is the same. Run `mise install` after editing to install any new tools.

### [`mise install`](/cli/install)

`mise install` installs tools but does not activate them—it downloads/builds/compiles the tool
into `~/.local/share/mise/installs`, but you can't use it until you "set" the version
in a `mise.toml` or `.tool-versions` file.

::: tip
If you're coming from `asdf`, there is no need to run `mise plugin add` first to install
the plugin; that happens automatically if needed. You can still install plugins manually
if you wish, or if you want to use a plugin that isn't in the default registry.
:::

It can be used in many ways:

- `mise install node@20.0.0` - install a specific version
- `mise install node@20` - install the latest version matching this prefix
- `mise install node` - install whatever version of node is currently specified in `mise.toml` (or other
  config files)
- `mise install` - install all plugins and tools specified in the config files
- `mise install --include-task-tools` - also install every tool required by tasks in the current
  scope without running those tasks

The last form is useful for warming CI, container, or offline caches before running any task. Add
`--monorepo` to include task tools from every configured monorepo root.

### [`mise exec`|`mise x`](/cli/exec)

Use `mise x` for one-off commands with specific tools. For example, to run a script
with Python 3.12:

```sh
mise x python@3.12 -- ./myscript.py
```

With the default [`auto_install`](/configuration/settings.html#auto_install) and
[`exec_auto_install`](/configuration/settings.html#exec_auto_install) settings, Python is installed
if it isn't already. `mise x` also reads local/global `mise.toml`/`.tool-versions` files,
so if you don't want to use `mise activate` or shims, you can use mise by prefixing
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

Similarly, `mise run` [executes tasks](/tasks/) and also activates the mise
environment with all of your tools.

## Auto-Install Mechanisms

mise provides several mechanisms to automatically install missing tools or versions as needed. Below, these are grouped by how and when they are triggered, with relevant settings for each. All mechanisms require the global [auto_install](/configuration/settings.html#auto_install) setting to be enabled (**all auto_install settings are enabled by default**).

### On-Demand Execution ([`mise x`](/cli/exec), [`mise r`](/cli/run))

When you run a command like [`mise x`](/cli/exec) or [`mise r`](/cli/run), mise automatically installs any missing tool versions required to execute the command.

- **When it triggers:** Whenever you use [`mise x`](/cli/exec) or [`mise r`](/cli/run) with a tool/version that is not yet installed.
- **How to control:**
  - Setting: [`exec_auto_install`](/configuration/settings.html#exec_auto_install) (default: true)
  - Setting: [`task_auto_install`](/configuration/settings.html#task_auto_install) (default: true)

### Command Not Found Handler (Shell Integration)

If you type a command in your shell (e.g., `node`) and it is not found, mise can attempt to auto-install the missing tool version if it knows which tool provides that binary.

- **When it triggers:** When a command is not found in the shell and the handler is enabled.
- **How to control:**
  - Setting: [`not_found_auto_install`](/configuration/settings.html#not_found_auto_install) (default: true)
- **Limitation:** mise identifies the provider from the registry's bin metadata, so this covers configured tools even if they have never been installed — but not tools configured by a raw backend spec (e.g. `cargo:some-crate`), which carry no such metadata. Install those explicitly with `mise install`, or `mise x` to install and run in one step. See [troubleshooting](/troubleshooting.html#auto-install-on-command-not-found-does-not-trigger).

::: tip
Disable auto_install for specific tools by setting [`auto_install_disable_tools`](/configuration/settings.html#auto_install_disable_tools) to a list of tool names.
:::
