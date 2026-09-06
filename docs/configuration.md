# Configuration

A project's `mise.toml` declares tools, environment variables, and tasks. Global
configuration supplies personal defaults; project and local files override them.

Start with one file in the project root:

```toml [mise.toml]
[tools]
node = "24"

[env]
NODE_ENV = "development"

[tasks.hello]
run = "node --eval 'console.log(process.env.NODE_ENV)'"
```

Run `mise run hello` to install the declared tool if needed and print
`development`. Use `mise config` to see the active config files and
`mise ls --current` to see the selected tool versions.

| Configure                                  | Reference                                               |
| ------------------------------------------ | ------------------------------------------------------- |
| Tool versions and installation options     | [Dev tools](/dev-tools/)                                |
| Variables passed to commands               | [Environments](/environments/)                          |
| Reusable values inside templates           | [Variables](/configuration/vars.html)                   |
| Development, test, and production overlays | [Config Environments](/configuration/environments.html) |
| Commands and dependencies                  | [Tasks](/tasks/)                                        |
| mise's own behavior                        | [Settings](/configuration/settings.html)                |

## `mise.toml`

`mise.toml` is the config file for mise. It can live at any of the following paths (in order of precedence; files higher in the list override those lower down):

- `mise.local.toml` - used for local config; this should not be committed to source control
- `mise.toml`
- `mise/config.toml`
- `mise/conf.d/*.toml` - all non-hidden TOML files in this directory are loaded in alphabetical order; dotted names like `x.base.toml` are [being deprecated](/configuration/environments.html#conf-d-environments) and load only for the matching environment under `env_conf_d = true`
- `.mise/config.toml`
- `.mise/conf.d/*.toml` - all non-hidden TOML files in this directory are loaded in alphabetical order; dotted names like `x.base.toml` are [being deprecated](/configuration/environments.html#conf-d-environments) and load only for the matching environment under `env_conf_d = true`
- `.config/mise.toml` - use this to group config files in a common directory
- `.config/mise/config.toml`
- `.config/mise/conf.d/*.toml` - the same fragment loading and [deprecation](/configuration/environments.html#conf-d-environments) behavior under the grouped config directory

::: tip
Run [`mise config`](/cli/config.html) to see the order in which mise loads files on your setup. This is often
much easier than working through mise's rules.
:::

Notes:

- Paths that start with `mise` can be dotfiles, e.g. `.mise.toml` or `.mise/config.toml`.
- This list doesn't include [Configuration Environments](/configuration/environments), which allow environment-specific config files like `mise.development.toml`—selected with `MISE_ENV=development`. Platform-specific environments like `mise.windows.toml` or `mise.macos-arm64.toml` can be enabled automatically with the [`auto_env` setting](/configuration/environments.html#platform-environments).
- See [`LOCAL_CONFIG_FILENAMES` in `src/config/mod.rs`](https://github.com/jdx/mise/blob/main/src/config/mod.rs) for the actual code for these paths and their precedence. Some legacy paths are not listed here for brevity.

## Configuration Hierarchy

mise uses a hierarchical configuration system that merges settings from multiple sources. Understanding this hierarchy helps you organize your development environments.

### How Configuration Merging Works

mise looks for these files in every parent directory, so if you have a `~/src/work/myproj/mise.toml` file,
what is defined there overrides anything set in
`~/src/work/mise.toml` or `~/.config/mise.toml`. The config contents are merged.

### Configuration Resolution Process

When mise needs configuration, it follows this process:

1. Reads early configuration, including the selected config environments.
2. Discovers system and global config, then searches the current directory and its
   parents up to the root or `MISE_CEILING_PATHS`.
3. Includes matching environment-specific files at each level of that hierarchy.
4. Merges the files with child directories taking precedence over parents, and
   same-directory variants following the order above.

An environment-specific parent file does not override an ordinary child file
just because it names an environment. Environment selection is part of file
discovery, not a final override applied after the hierarchy.

### Visual Configuration Hierarchy

```
/
├── etc/mise/                         # System-wide config (lowest precedence)
│   ├── conf.d/*.toml                 # System fragments, loaded alphabetically
│   ├── config.toml                   # System defaults
│   └── config.<env>.toml             # Env-specific system config (MISE_ENV or -E)
└── home/user/
    ├── .config/mise/
    │   ├── conf.d/*.toml             # User fragments, loaded alphabetically
    │   ├── config.toml               # Global user config
    │   ├── config.<env>.toml         # Env-specific user config
    │   ├── config.local.toml         # User-local overrides
    │   └── config.<env>.local.toml   # Env-specific user-local overrides
    └── work/
        ├── mise.toml                 # Work-wide settings
        └── myproject/
            ├── mise.local.toml       # Local overrides (git-ignored)
            ├── mise.toml             # Project config
            ├── mise/
            │   ├── config.toml       # Visible grouped project config
            │   └── conf.d/*.toml     # Visible project fragments, loaded alphabetically
            ├── .mise/
            │   ├── config.toml       # Project config grouped under .mise
            │   └── conf.d/*.toml     # Project fragments, loaded alphabetically
            ├── mise.<env>.toml       # Env-specific project config
            ├── mise.<env>.local.toml # Env-specific project local overrides
            └── backend/
                └── mise.toml         # Service-specific config (highest precedence)
```

### Merge Behavior by Section

Different configuration sections merge in different ways:

**Tools** (`[tools]`): Additive with overrides

```toml
# Global: node@18, python@3.11
# Project: node@20, go@1.21
# Result: node@20, python@3.11, go@1.21
```

**Tool policy** (`[tool_config]`): Applies only to tools declared by configs
sharing the same config root; it is not merged into invocation-wide settings

```toml
[tool_config]
locked = true
```

**Environment Variables** (`[env]`): Additive with overrides

```toml
# Global: NODE_ENV=development
# Project: NODE_ENV=production, API_URL=localhost
# Result: NODE_ENV=production, API_URL=localhost
```

**Tasks** (`[tasks]`): A more specific command definition replaces the earlier command

```toml
# Global: [tasks.test] = "npm test"
# Project: [tasks.test] = "yarn test"
# Result: "yarn test"
```

Metadata-only task definitions can overlay an existing task without replacing its
command. Included task files and file tasks have additional merge rules; see
[`task_config.includes`](/tasks/task-configuration.html#task_config.includes).

**Settings** (`[settings]`): Additive with overrides

```toml
# Global: experimental = true
# Project: jobs = 4
# Result: experimental = true, jobs = 4
```

::: tip
Run `mise config` to see what files mise has loaded in order of precedence.
:::

### Target File for Write Operations

When commands like [`mise use`](/cli/use), [`mise set`](/cli/set), or [`mise unuse`](/cli/unuse) need to write to a config file, they use the **lowest precedence file in the highest precedence directory**. This means:

- If both `mise.toml` and `mise.local.toml` exist, writes go to `mise.toml`
- If both `mise.toml` and `mise.production.toml` exist, writes go to `mise.toml`
- If only `mise.local.toml` exists, writes go to `mise.local.toml`

This behavior ensures that shared configuration (`mise.toml`) is updated by default, while local overrides (`mise.local.toml`) and environment-specific configs remain untouched unless explicitly targeted.

::: info Example

```bash
# With both mise.toml and mise.local.toml present:
$ mise use node@22              # writes to mise.toml
$ mise use --env local node@20  # writes to mise.local.toml
$ mise set NODE_ENV=production  # writes to mise.toml
```

:::

### `[tools]` - Dev tools

See [Tools](/dev-tools/). In addition to specifying versions, each tool entry can include options such as:

- `os`: Restrict installation to certain operating systems
- `depends`: Install order relative to other tools in this config only; vfox plugin hook dependencies belong in plugin `metadata.lua` (see [Tool Dependencies](/dev-tools/#tool-dependencies))
- `install_env`: Environment vars used during download, install, and tool-level `postinstall`
- `postinstall`: Command to run after installation completes for that specific tool

Examples:

```toml
[tools]
node = { version = "22", postinstall = "corepack enable" }
```

### `[tool_config]` - Config-root-scoped tool policy

`[tool_config]` applies policy to tools declared by configs sharing the same
config root. For example, policy in `mise.local.toml` also applies to tools in
`mise.toml` beside it. It does not affect tools inherited from global, system,
or parent config roots.

```toml
[tool_config]
locked = true

[tools]
node = "24"
```

Currently, `locked` is the only supported policy. It requires this config
root's tools to resolve and install from their lockfiles. See [mise.lock](/dev-tools/mise-lock.html#strict-lockfile-mode).

### `[env]` - Arbitrary Environment Variables

See [environments](/environments/).

### `[vars]` - Configuration Variables

Define values that can be reused in Tera-rendered configuration without exporting them to child
processes. See [Variables](/configuration/vars).

### `[tasks.*]` - Run files or shell scripts

See [Tasks](/tasks/).

### `[settings]` - Mise Settings

See [Settings](/configuration/settings) for the full list of settings.

### `[plugins]` - Specify Custom Plugin Repository URLs

Use `[plugins]` to add or modify plugin shortnames. This only affects
_new_ plugin installations; existing plugins can use any URL.

```toml
[plugins]
elixir = "https://github.com/my-org/mise-elixir.git"
node = "https://github.com/my-org/mise-node.git#DEADBEEF" # supports specific gitref
"vfox-backend:myplugin" = "https://github.com/jdx/vfox-npm"
```

The plugin type prefix (e.g., `asdf:`, `vfox:` or `vfox-backend:`) is optional.
If omitted, mise clones the plugin first and then detects the plugin type from
the installed plugin files.

To install a plugin from a specific URL once, use
`mise plugin install <NAME> <GIT_URL>` instead. Add this section to `mise.toml` when you want
to share the plugin location and revision with other developers in your project.

Local plugin directories are also supported. Absolute paths and paths beginning
with `~/` are used directly. Explicit relative paths beginning with `./` or `../`
are resolved relative to the config root of the file that declares them:

```toml
[plugins]
example = "./plugins/mise-example"
```

Local plugins are symlinked into mise's plugin directory, matching
`mise plugins link`, so changes to the source directory are available immediately.
As with remote entries, `[plugins]` only affects new installations. Run
`mise plugins install --force <NAME>` to replace an existing plugin with the
configured local source. `file://` sources remain Git repositories and are cloned.

This replaces the deprecated `settings.shorthands_file` / `MISE_SHORTHANDS_FILE` mechanism: put the
same `shortname = "backend-or-url"` entries under `[plugins]` instead of a separate TOML file.

### `[tool_alias]` - Tool version aliases

::: tip
`[alias]` has been renamed to `[tool_alias]` to distinguish it from `[shell_alias]`.
The old `[alias]` key still works but is deprecated.
:::

The following makes `mise install node@my_custom_node` install node-20.x.
Aliases can also be specified in a [plugin](/dev-tools/aliases.md).

```toml
[tool_alias.node.versions]
my_custom_node = '20'
```

### `[shell_alias]` - Shell aliases

Define shell aliases that are set when entering a directory and unset when leaving:

```toml
[shell_alias]
ll = "ls -la"
gs = "git status"
dev = "npm run dev"
```

These work similarly to environment variables—they're set dynamically based on your current directory.
See [Shell Aliases](/shell-aliases) for more details.

### Minimum mise version

Specify the minimum mise version required by the configuration file.

You can set a hard minimum (errors if unmet) or a soft minimum (warns and continues):

```toml
# Require this version or newer
min_version = '2024.11.1'
```

Or specify a hard minimum and a newer recommended version:

```toml
min_version = { hard = '2024.11.1', soft = '2026.1.0' }
```

When a soft minimum is not met, mise prints a warning and, if available, self-update instructions. When a hard minimum is not met, mise errors and shows self-update instructions.

Use a hard minimum for syntax or behavior the project requires. A soft minimum
recommends an upgrade while allowing older clients to continue. A soft-only
requirement is also valid: `min_version = { soft = '2026.1.0' }`.

Keep mise current so backend integrations and deprecation notices stay up to date.
A minimum version lets teammates upgrade without changing the project's requirement.

### Monorepo root

Mark a configuration file as a monorepo root to enable target path syntax for tasks.

```toml
monorepo_root = true

[monorepo]
config_roots = ["projects/frontend", "projects/api"]
```

`monorepo_root` enables task addressing; `config_roots` identifies the projects to
load. When enabled:

- Tasks in subdirectories are available with namespaced paths (e.g., `//projects/frontend:build`)
- Subdirectory tasks use tools from parent configs
- Tasks are only loaded when needed (e.g., when running them, or with `mise tasks ls --all`)
- Trusting a monorepo root allows descendant configs to share that trust; review
  the repository before trusting it (see [trust behavior](/cli/trust.html))

See [Monorepo Tasks](/tasks/monorepo) for detailed usage and examples.

### `mise.toml` schema

- You can find the JSON schema for `mise.toml` in [schema/mise.json](https://github.com/jdx/mise/blob/main/schema/mise.json) or at <https://mise.jdx.dev/schema/mise.json>.
- Some editors can load it automatically to provide autocompletion and validation when editing a `mise.toml` file ([VSCode](https://code.visualstudio.com/docs/languages/json#_json-schemas-and-settings), [IntelliJ](https://www.jetbrains.com/help/idea/json.html#ws_json_using_schemas), [neovim](https://github.com/b0o/SchemaStore.nvim), etc.). It is also available in the [JSON schema store](https://www.schemastore.org/).
- `included tasks` (see [task configuration](/tasks/task-configuration)) use a separate schema: <https://mise.jdx.dev/schema/mise-task.json>

## Global config: `~/.config/mise/config.toml`

mise can be configured in `~/.config/mise/config.toml`. It works like a local `mise.toml`, but
applies to every directory.

Only a few common settings are shown here. See [Settings](/configuration/settings) for the full
list and descriptions.

```toml [~/.config/mise/config.toml]
[tools]
# global tool versions go here
# you can set these with `mise use -g`
node = 'lts'
python = ['3.10', '3.11']

[settings]
# read version files used by other version managers, such as .nvmrc
idiomatic_version_file_enable_tools = ['node']

trusted_config_paths = [
    '~/work/my-trusted-projects',
]

env_file = '.env' # load env vars from a dotenv file, see `MISE_ENV_FILE`

[settings.status]
show_env = false
show_tools = false

# "_" is a special key for information you'd like to put into mise.toml that mise will never parse
[_]
foo = "bar"
```

## System config: `/etc/mise/config.toml`

Like `~/.config/mise/config.toml`, but applied to all users on the system. This is useful for
setting system-wide defaults.

## `.tool-versions`

The `.tool-versions` file is asdf's config file, and mise can use it just like `mise.toml`.
It isn't as flexible, so `mise.toml` is recommended instead. It is useful if you
already have many `.tool-versions` files or work on a team that uses asdf.

Here is an example with all the supported syntax:

```text
node        20.0.0       # comments are allowed
ruby        3            # can be fuzzy version
shellcheck  latest       # also supports "latest"
jq          1.6
erlang      ref:master   # compile from vcs ref
go          prefix:1.19  # uses the latest 1.19.x version—needed in case "1.19" is an exact match
shfmt       path:./shfmt # use a custom runtime
node        lts          # use lts version of node (not supported by all plugins)

node        sub-2:lts      # subtract 2 from the resolved major version (e.g.: 20 becomes 18)
python      sub-0.1:latest # subtract 1 from the resolved minor version (e.g.: 3.11 becomes 3.10)
```

See [the asdf docs](https://asdf-vm.com/manage/configuration.html#tool-versions) for more info on
this file format.

## Scopes

Both `mise.toml` and `.tool-versions` support "scopes", which modify how a version is resolved:

- `ref:<SHA>` - compile from a vcs (usually git) ref
- `prefix:<PREFIX>` - use the latest version that matches the prefix. Useful for Go, since `1.20`
  would only match `1.20` exactly, whereas `prefix:1.20` matches `1.20.1`, `1.20.2`, etc.
- `path:<PATH>` - use a custom compiled version at the given path. One use case is reusing
  Homebrew tools (e.g. `path:/opt/homebrew/opt/node@20`). On Windows both separators work and
  mise stores the forward-slash form either way, but mind the TOML quoting: a backslash is an
  escape inside a _basic_ (double-quoted) string, so write `{ path = 'C:\tools\node' }` as a
  literal string, or double them as `"C:\\tools\\node"`. `"C:\tools\node"` is not rejected — TOML
  reads `\t` as a tab — so the path silently becomes something else. A path containing a `cmd.exe`
  metacharacter (`& | < > ^ %`) is rejected there, since the path is passed to tool plugins that
  build shell commands with it; `%` in particular is not a literal.
- `sub-<PARTIAL_VERSION>:<ORIG_VERSION>` - resolves `ORIG_VERSION`, subtracts the numeric components
  in `PARTIAL_VERSION` from the corresponding resolved version components, then resolves the result
  as a version prefix. For example, `sub-2:lts` resolves `lts` and subtracts 2 from its major
  component (`20` becomes `18`), while `sub-0.1:latest` subtracts 1 from the resolved minor
  component (`3.11` becomes `3.10`). This is numeric version arithmetic, not a request for the Nth
  previous release.

## Idiomatic version files

mise supports "idiomatic version files" just like asdf. They're language-specific files
like `.node-version` and `.python-version`. These are ideal for setting the runtime version of a project without forcing
other developers to use a specific tool like mise or asdf.

They support aliases, so an `.nvmrc` file containing `lts/hydrogen` works
in both mise and nvm. Here are some of the supported idiomatic version files:

<!-- mise:idiomatic-version-files:start -->

| Plugin        | Idiomatic Files                                                                                                                                                                                                                                                                                            |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| atmos         | `.atmos-version`                                                                                                                                                                                                                                                                                           |
| bun           | `.bun-version`, `package.json`                                                                                                                                                                                                                                                                             |
| chezmoi       | `.chezmoiversion`                                                                                                                                                                                                                                                                                          |
| cmake         | `CMakeLists.txt`                                                                                                                                                                                                                                                                                           |
| crystal       | `.crystal-version`                                                                                                                                                                                                                                                                                         |
| dagger        | `dagger.json`                                                                                                                                                                                                                                                                                              |
| deno          | `.deno-version`, `package.json`                                                                                                                                                                                                                                                                            |
| dotnet        | `global.json`                                                                                                                                                                                                                                                                                              |
| earthly       | `Earthfile`                                                                                                                                                                                                                                                                                                |
| elixir        | `.exenv-version`                                                                                                                                                                                                                                                                                           |
| go            | `.go-version`, `go.mod`                                                                                                                                                                                                                                                                                    |
| golangci-lint | `.golangci.yml`, `.golangci.yaml`, `.golangci.toml`, `.golangci.json`                                                                                                                                                                                                                                      |
| goreleaser    | `.config/goreleaser.yml`, `.config/goreleaser.yaml`, `.goreleaser.yml`, `.goreleaser.yaml`, `goreleaser.yml`, `goreleaser.yaml`                                                                                                                                                                            |
| java          | `.java-version`, `.sdkmanrc`                                                                                                                                                                                                                                                                               |
| lefthook      | `lefthook.yml`, `lefthook.yaml`, `.lefthook.yml`, `.lefthook.yaml`, `lefthook.toml`, `.lefthook.toml`, `lefthook.json`, `.lefthook.json`, `lefthook.jsonc`, `.lefthook.jsonc`, `.config/lefthook.yml`, `.config/lefthook.yaml`, `.config/lefthook.toml`, `.config/lefthook.json`, `.config/lefthook.jsonc` |
| node          | `.nvmrc`, `.node-version`, `package.json`                                                                                                                                                                                                                                                                  |
| npm           | `package.json`                                                                                                                                                                                                                                                                                             |
| opentofu      | `.opentofu-version`                                                                                                                                                                                                                                                                                        |
| packer        | `.packer-version`                                                                                                                                                                                                                                                                                          |
| perl          | `.perl-version`                                                                                                                                                                                                                                                                                            |
| pixi          | `pixi.toml`, `pyproject.toml`                                                                                                                                                                                                                                                                              |
| pnpm          | `package.json`                                                                                                                                                                                                                                                                                             |
| pre-commit    | `.pre-commit-config.yaml`                                                                                                                                                                                                                                                                                  |
| python        | `.python-version`, `.python-versions`                                                                                                                                                                                                                                                                      |
| ruby          | `.ruby-version`, `Gemfile`                                                                                                                                                                                                                                                                                 |
| ruff          | `ruff.toml`, `.ruff.toml`                                                                                                                                                                                                                                                                                  |
| rust          | `rust-toolchain.toml`                                                                                                                                                                                                                                                                                      |
| swift         | `.swift-version`                                                                                                                                                                                                                                                                                           |
| task          | `Taskfile.yml`, `Taskfile.yaml`, `taskfile.yml`, `taskfile.yaml`                                                                                                                                                                                                                                           |
| terraform     | `.terraform-version`                                                                                                                                                                                                                                                                                       |
| terragrunt    | `.terragrunt-version`                                                                                                                                                                                                                                                                                      |
| terramate     | `.terramate-version`                                                                                                                                                                                                                                                                                       |
| yarn          | `.yvmrc`, `package.json`                                                                                                                                                                                                                                                                                   |
| zig           | `.zig-version`                                                                                                                                                                                                                                                                                             |

<!-- mise:idiomatic-version-files:end -->

Registry-backed tools can also describe how mise should extract versions from structured
idiomatic files. Registry entries may use the same `version_regex`, `version_json_path`, and
`version_expr` parsers as the [HTTP backend](/dev-tools/backends/http.html#version-listing).
This lets tools installed through backends such as `aqua:` and `github:` support JSON manifests
and other tool-specific version files without requiring an asdf or vfox plugin.

### Which fields mise reads

An idiomatic version file is only read for fields that declare **the version the project is built
with**. Fields that declare a **minimum compatible version** — a floor for whoever consumes the
project — are not version requests and mise does not install from them. A floor says nothing about
which version the project is developed and tested against: a library that still supports Node 18
or CMake 3.25 is almost certainly not built with it, so resolving the floor either pins everyone to
the oldest supported release or, read as a range, means "latest".

A configuration-format major is different and is still read: a GoReleaser config `version: 2` is a
schema selector deliberately coupled to the CLI major, not a compatibility floor, so it selects the
latest GoReleaser 2.x.

::: warning
mise used to treat two floors as version requests. Both are deprecated, warn when they resolve a
version, and will be removed in mise 2026.11.0: `go.mod`'s `go X.Y` directive (add a
`toolchain goX.Y.Z` line to `go.mod`, or use `.go-version` or `mise.toml`) and `CMakeLists.txt`'s
`cmake_minimum_required` (use `mise.toml`).

A project that has already migrated can opt into the final behavior — floors ignored, no warning —
before then:

```sh
mise settings set idiomatic_version_file_ignore_minimum_versions true
```

That setting is removed in 2026.11.0 along with the behavior it guards.
:::

For `package.json` (supported by `node`, `deno`, `bun`, `npm`, `pnpm`, and `yarn`):

- Runtime tools (`node`, `deno`, and `bun`) read `devEngines.runtime` (both single object and array formats are supported).
- Package managers (`npm`, `pnpm`, and `yarn`) read `devEngines.packageManager` or top-level `packageManager` (e.g. `pnpm@9.1.0` or `npm@10.0.0`).
- For `bun`, mise checks `devEngines.runtime` first, falling back to `devEngines.packageManager` and top-level `packageManager` (e.g. `bun@1.2.0`).

The `engines` field is **not** read, and this is the clearest case of the rule above. `engines`
declares the range of Node versions a package is _compatible_ with — npm uses it to warn or fail
when someone installs the package on an unsupported runtime. It is a statement about consumers, and
it is routinely a wide range (`>=18`) that no one develops against. `devEngines`, added by npm
precisely to fill this gap, states the version the project's own developers use, which is what mise
needs. If you only have `engines`, pin the real version explicitly:

```sh
mise use node@22
```

For `go.mod`, the `toolchain goX.Y.Z` directive is used — an exact pin of the toolchain the module
builds and tests with. The `go X.Y` directive is a minimum and is deprecated (see above).

### Enabling idiomatic version files

In mise, these are disabled by default; see <https://github.com/jdx/mise/discussions/4345> for the rationale.

- Run `mise settings add idiomatic_version_file_enable_tools python` to enable them for a specific tool such as Python ([docs](/configuration/settings.html#idiomatic_version_file_enable_tools))

Individual files can be disabled for a tool with a `tool:filename` pair. For example, to use
`.nvmrc` for node while leaving `package.json` available to package managers:

```sh
mise settings add idiomatic_version_file_disable_files node:package.json
```

There is a small performance cost to discovering and parsing these files. Registry parsers run
in-process; plugin-provided files may invoke the plugin's parser. Results are [cached](/cache-behavior),
so this is generally not noticeable.

asdf calls these "legacy version files". mise uses "idiomatic version files" to
distinguish language and ecosystem conventions from mise's own configuration.

## Settings

See [Settings](/configuration/settings) for the full list of settings.

## Tasks

See [Tasks](/tasks/) for the full list of configuration options.

## Environment variables

::: tip
Most environment variables in mise set [settings](/configuration/settings), so they are documented
there. The following environment variables are not settings.

A setting in mise is generally something that can be configured either as an environment variable
or set in a config file.
:::

mise can also be configured via environment variables. The following options are available:

### `MISE_DATA_DIR`

Default (Linux): `~/.local/share/mise` or `$XDG_DATA_HOME/mise`
Default (macOS): `~/.local/share/mise` or `$XDG_DATA_HOME/mise`
Default (Windows): `%LOCALAPPDATA%\mise` or `$XDG_DATA_HOME/mise`

This is the directory where mise stores plugins and tool installs. These should not be shared
across machines.

### `MISE_CACHE_DIR`

Default (Linux): `~/.cache/mise` or `$XDG_CACHE_HOME/mise`
Default (macOS): `~/Library/Caches/mise` or `$XDG_CACHE_HOME/mise`
Default (Windows): `%TEMP%\mise` or `$XDG_CACHE_HOME/mise`

This is the directory where mise stores its internal cache. It should not be shared
across machines and may be deleted whenever mise is not running.

### `MISE_TMP_DIR`

Default: [`std::env::temp_dir()`](https://doc.rust-lang.org/std/env/fn.temp_dir.html) implementation
in Rust

This is used for temporary storage, such as when installing tools.

### `MISE_SYSTEM_CONFIG_DIR`

Default: `/etc/mise`

This is the directory where mise stores system-wide configuration.
`MISE_SYSTEM_DIR` is also supported as a legacy alias.

### `MISE_GLOBAL_CONFIG_FILE`

Default: `$MISE_CONFIG_DIR/config.toml` (usually `~/.config/mise/config.toml`)

This is the path to the global config file.

Use this when you want global writes, such as `mise use` or `mise set` run from
`$HOME`, to target a different config file. [`MISE_DEFAULT_CONFIG_FILENAME`](#mise_default_config_filename)
customizes the default local config filename, not the global config path.

### `MISE_DEFAULT_CONFIG_FILENAME`

Default: `mise.toml`

This customizes the default local config filename used when mise creates or
looks for project config files.

### `MISE_GLOBAL_CONFIG_ROOT`

Default: `$HOME`

::: v-pre
This is the path which is used as `{{config_root}}` for the global config file.
:::

### `MISE_ENV_FILE`

Set to a filename to read env vars from a dotenv file, e.g. `MISE_ENV_FILE=.env`.
mise searches for and loads all matching files in the current directory and its parents.
This uses [dotenvy](https://crates.io/crates/dotenvy) under the hood.

### `MISE_${TOOL}_VERSION`

Set the version for a tool. For example, `MISE_NODE_VERSION=20` will use <node@20.x> regardless
of what is set in `mise.toml`/`.tool-versions`.

### `MISE_TRUSTED_CONFIG_PATHS`

This is a list of paths that mise will automatically mark as
trusted. They are separated according to platform conventions for the PATH
environment variable: `:` on Unix and `;` on Windows.

### `MISE_CEILING_PATHS`

This is a list of paths at which mise stops searching for
configuration files and file tasks. This is useful to keep
mise from searching slow-loading directories. Paths are separated according to platform conventions for the PATH environment variable: `:` on Unix and `;` on Windows.

### `MISE_LOG_LEVEL=trace|debug|info|warn|error`

Sets the verbosity of mise's log output.

You can also use `MISE_DEBUG=1`, `MISE_TRACE=1`, and `MISE_QUIET=1` as well as
`--log-level=trace|debug|info|warn|error`.

### `MISE_LOG_FILE=~/mise.log`

Output logs to a file.

### `MISE_LOG_FILE_LEVEL=trace|debug|info|warn|error`

Same as `MISE_LOG_LEVEL`, but for the log _file_. This is useful if you want
to store logs without cluttering your display.

### `MISE_LOG_HTTP=1`

Display HTTP requests/responses in the logs.

### `MISE_LOG_VERBOSE_DEPS=1`

Debug and trace logs from noisy third-party crates (`h2`, `hyper`,
`reqwest`, `rustls`, etc., which emit a line per HTTP/2 frame or socket
read) are always dropped — they would otherwise overwhelm debug/trace
output. Set this to `1` to let those logs through; it is the only way to
see them, including under `--log-level=trace`/`-vv`.

### `MISE_QUIET=1`

Equivalent to `MISE_LOG_LEVEL=warn`.

### `MISE_HTTP_TIMEOUT`

Set the timeout for HTTP requests in seconds. The default is `30`.

### `MISE_RAW=1`

Set to "1" to connect plugin scripts directly to stdin/stdout/stderr. By default stdin is disabled
because when several plugins install in parallel you wouldn't see the prompt. Use this if a
plugin accepts input or otherwise does not seem to install correctly.

This also sets `MISE_JOBS=1`, because only one plugin script can run at a time.

### `MISE_TERM_WIDTH`

Override the terminal width mise uses to render tables and lists (e.g. `mise ls`).
By default mise detects the width from the terminal. This is useful in CI or other
non-interactive environments where detection returns a bogus value (for example
CircleCI, where the width is reported as `0`), producing oddly wrapped output.

If `MISE_TERM_WIDTH` is unset, mise falls back to the conventional `COLUMNS`
environment variable, and finally to auto-detection. The override is honored
exactly, so you can also force a narrower width:

```sh
MISE_TERM_WIDTH=120 mise ls
```

### `MISE_FISH_AUTO_ACTIVATE=1`

Controls whether the `vendor_conf.d` script for fish automatically activates mise.
Homebrew and potentially other installs use this file to activate mise without
any configuration.

Enabled by default; set to "0" to disable.
