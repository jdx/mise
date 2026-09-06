# Dev Tools

mise installs development tools and selects their versions for each project.
Keep multiple versions of Node.js, Python, Ruby, Go, and other tools on the same
machine, then declare which ones a project uses in `mise.toml`.

## Add a tool to a project

From the project directory:

```sh
mise use node@24 python@3.13
```

This installs the tools and records the version requests in your configuration:

```toml [mise.toml]
[tools]
node = "24"
python = "3.13"
```

Run a command with those tools:

```sh
mise exec -- node --version
```

With [shell activation](/getting-started.html#activate-mise), you can run
`node --version` directly. mise updates your shell environment as you move between
projects. Activation selects installed tools; use `mise install` to install tools
after cloning a repository or editing its configuration.

## Choose the right command

| Goal                                             | Command                               |
| ------------------------------------------------ | ------------------------------------- |
| Add or change a project's tool version           | `mise use node@24`                    |
| Set a personal default                           | `mise use --global node@24`           |
| Install tools declared by a project              | `mise install`                        |
| Try a version without saving it                  | `mise exec node@24 -- node --version` |
| Show tools selected by the current configuration | `mise ls --current`                   |
| Upgrade within the configured version request    | `mise upgrade node`                   |

A version request such as `"24"` selects a release in that series. An exact pin
selects a specific release. See [mise.lock](/dev-tools/mise-lock.html) for recording
resolved versions without replacing the version requests in `mise.toml`.

## How tools are selected

1. mise discovers configuration in the current directory and its parents, along
   with global configuration. More specific configuration can override defaults.
2. Each tool's [backend](/dev-tools/backends/) resolves its version request and
   handles installation. The [registry](/registry.html) maps short tool names to
   backends, so you usually don't need to choose one yourself.
3. mise adds the selected tools to the command's `PATH`. By default, `mise exec`
   and `mise run` install missing tools before executing the command or task.

Use `mise config ls` to inspect active configuration files. See
[configuration](/configuration.html) for the full precedence rules.

### Shells, editors, and scripts

- **Interactive shells:** [activate mise](/getting-started.html#activate-mise) to
  update `PATH` and project environment variables at each prompt.
- **Editors:** use [IDE integration](/ide-integration.html), including
  [shims](/dev-tools/shims.html) where a program needs a stable executable path.
- **Scripts and CI:** use `mise exec -- <command>` or `mise run <task>` to load the
  project environment without relying on shell startup files.

### Existing version files

mise also reads asdf `.tool-versions` files. Tool-specific files such as `.nvmrc`
and `.python-version` require enabling
[idiomatic version files](/configuration.html#idiomatic-version-files).
For migration guidance, see [comparison to asdf](./comparison-to-asdf).

### Templates in tool configuration

Tool versions and options can reference environment variables and
[`vars`](/configuration/vars.html), including values from `_.source`, `_.file`,
and environment modules. Those values are resolved before tool templates render.

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

- **Version lists**: Cached for 1 hour by default ([`fetch_remote_versions_cache`](/configuration/settings.html#fetch_remote_versions_cache)) to avoid repeated API calls
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
