---
description: "Set options on a tool in mise.toml or on the command line, such as its platforms, install dependencies, install environment, and postinstall command."
---

# Tool options

A tool option is a key you set on one tool's entry in `[tools]`, next to its
version. Some options work with every tool, such as `os`, `depends`, and
`postinstall`; the rest belong to the tool's backend, such as `asset_pattern`
for `github:` tools. Set one when a tool needs more than a version: to install
it only on some platforms, to run a command after it installs, or to tell the
backend which file to download.

```toml [mise.toml]
[tools]
# install ripgrep only on Linux and macOS
ripgrep = { version = "15", os = ["linux", "macos"] }
# run a command each time mise installs a Node.js version
node = { version = "24", postinstall = "corepack enable" }
```

## Options every tool accepts {#shared-options}

These options work with any tool entry, except where a row names the backends
that support them. Options that belong to one backend, such as `matching` for
GitHub, are listed on each [backend page](/dev-tools/backends/), and the options
of a core tool, such as Node.js, on its [language page](/core-tools.html).

| Option                | What it does                                                                                                                                                                                                                                                                                      |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `version`             | The [version request](/dev-tools/versions.html). Use `prefix`, `ref`, or `path` instead for [those kinds of request](/dev-tools/versions.html#scopes).                                                                                                                                            |
| `os`                  | Install and use the tool only on these platforms; see [OS-specific tools](#os-specific-tools).                                                                                                                                                                                                    |
| `depends`             | Install these tools first; see [Tool dependencies](#tool-dependencies).                                                                                                                                                                                                                           |
| `install_env`         | Environment variables for the commands that install the tool (build scripts, package managers, plugin hooks) and for its `postinstall`. mise also reads GitHub, GitLab, and Forgejo token variables from it when it resolves and downloads the tool; see [tokens](/dev-tools/github-tokens.html). |
| `postinstall`         | A command to run after the tool installs; see [Tool postinstall commands](#tool-postinstall-commands).                                                                                                                                                                                            |
| `lazy`, `lazy_bins`   | Install the tool the first time one of its commands runs; see [Lazy tools](/dev-tools/shims.html#lazy-tools).                                                                                                                                                                                     |
| `auto_update`         | In global config only, update the tool before it runs; see [Automatic tool updates](/dev-tools/#automatic-tool-updates).                                                                                                                                                                          |
| `minimum_release_age` | Select only versions released at least this long ago (a duration such as `3d`) or before a date. A version with no release date is not held back, and mise warns when it installs one; see [minimum release age](/security.html#minimum-release-age).                                             |
| `bump`                | Set `false` to keep the tool out of `mise upgrade --bump` and `mise outdated --bump`: it is upgraded within its request and mise.toml is not rewritten. Naming the tool (`mise upgrade node --bump`) still bumps it. Not available in `.tool-versions`.                                           |
| `prerelease`          | Include prereleases in `mise ls-remote` and when resolving `latest` or a prefix. Every backend honors it except the core `java` plugin; see [`prereleases`](/configuration/settings.html#prereleases).                                                                                            |
| `version_order`       | Order versions by `semver` or by `source` on aqua, github, gitlab, forgejo, and http tools, or by `published_at` on github, gitlab, and forgejo tools; see [Version ordering](/dev-tools/versions.html#version-ordering).                                                                         |

mise's HTTP client does not read proxy variables from `install_env`, so set
`HTTPS_PROXY` and similar variables in the environment that starts mise.

## Unknown options {#unknown-options}

mise does not check option names when it reads `mise.toml`. The tool's backend
checks them when mise installs the tool, and what it does with an option it
does not read depends on the backend:

| Backend                                                                                                                       | An option the backend does not read                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo`, `conda`, `forgejo`, `gem`, `github`, `gitlab`, `go`, `http`, `npm`, `packslip`, `pypi`, `s3`, `spinel`, `spm`, `ubi` | Fails the install.                                                                                                                                                                  |
| `dotnet`, and [core tools](/core-tools.html) such as `node`                                                                   | Has no effect. mise warns and installs the tool.                                                                                                                                    |
| `aqua`                                                                                                                        | Is not checked: any other option is a [recipe variable](/dev-tools/backends/aqua.html#vars).                                                                                        |
| `asdf`, `vfox`, and tools from backend plugins                                                                                | Goes to the plugin, which decides what to do with it; see the [asdf](/dev-tools/backends/asdf.html#plugin-options) and [vfox](/dev-tools/backends/vfox.html#tool-options) backends. |

A failed install names the option and the backend. Here the `cargo` backend
is given `asset_pattern`, an option of the `github` backend:

```text
mise ERROR Failed to install cargo:eza@0.23.4: cargo:eza: option `asset_pattern` is not used by the cargo backend
hint: Remove it, or check the spelling against the backend's documented options
```

When the backend reads an option with a similar name, the hint suggests it.
Here `asset_pattern` is misspelled on a `github` tool:

```text
mise ERROR Failed to install github:cli/cli@2.60.0: github:cli/cli: option `asset_patern` is not used by the github backend
hint: Did you mean `asset_pattern`?
```

The warning from a core tool names the option in the same way:

```text
mise WARN  node: option `lazy_bin` is not used by the core backend, so it has no effect (did you mean `lazy_bins`?)
```

The check applies however the option was written: in `mise.toml`, in brackets
on the command line, or inside a `platforms` table. `mise install --dry-run`
runs it without installing anything. Commands that install nothing, such as
`mise ls`, do not.

The [options every tool accepts](#shared-options) are different, because mise
reads them itself. For most of them a value of the wrong type, such as
`lazy = "yes"`, is an error as soon as mise reads the config.

## Inline or table syntax {#table-format}

Short options fit on one line; use a table when a tool has several. These are
equivalent:

::: code-group

```toml [Inline]
[tools]
ripgrep = { version = "15", os = ["linux", "macos"] }
```

```toml [Table]
[tools.ripgrep]
version = "15"
os = ["linux", "macos"]
```

:::

mise reads TOML 1.1, so an inline table can span several lines and end with a
trailing comma. Nested options, such as the HTTP backend's per-platform
`platforms`, can use dotted keys or one line per platform:

```toml [mise.toml]
[tools."http:my-tool"]
version = "1.0.0"
platforms.macos-arm64.url = "https://example.com/my-tool-macos-arm64.tar.gz"
platforms.linux-x64.url = "https://example.com/my-tool-linux-x64.tar.gz"
```

See the [HTTP backend](/dev-tools/backends/http.html) for checksums and
executable selection.

## Options on the command line {#on-the-command-line}

Append options in brackets to the tool name, separated by commas, before the
version. Quote the argument so the shell does not expand the brackets:

```sh
mise use 'github:jqlang/jq[version_prefix=jq-,rename_exe=jq]@1.8.2'
```

The same form works with `mise install` and `mise exec`. `mise use` writes the
options to `mise.toml`:

```toml [mise.toml]
[tools]
"github:jqlang/jq" = { version = "1.8.2", version_prefix = "jq-", rename_exe = "jq" }
```

`mise use --tool-option KEY=VALUE` sets an option for the tool that follows
it. `mise use --tool-option mr_boxington=true rust mr-boxington` sets the
option on `rust` and adds the `mr-boxington` tool, which the option needs; see
[Cache Cargo builds with Mr Boxington](/lang/rust.html#share-cargo-builds-with-mr-boxington).

## Variables in tool options {#templates-in-tool-configuration}

Versions and option values can use [templates](/templates.html) with
environment variables and [`vars`](/configuration/vars.html), including values
that `[env]` loads with `_.source`, `_.file`, or environment modules. For
example, point a Go install at your company's module proxy:

```toml [mise.toml]
[vars]
goproxy = "https://goproxy.example.com"

[tools]
go = "1.27"
"go:github.com/mikefarah/yq/v4" = { version = "latest", install_env = { GOPROXY = "{{ vars.goproxy }}" } }
```

## OS-specific tools {#os-specific-tools}

Set `os` to install and use a tool only on some platforms. Everywhere else,
mise skips the tool:

```toml [mise.toml]
[tools]
# Linux and macOS only
ripgrep = { version = "latest", os = ["linux", "macos"] }

# Windows only
"github:PowerShell/PowerShell" = { version = "latest", os = ["windows"] }

# Linux, and macOS on Apple silicon
hk = { version = "latest", os = ["linux", "macos/arm64"] }
```

Values are `linux`, `macos` (or `darwin`), `windows` (or `win`), and `unix`
(every platform except Windows). Add an architecture to narrow a value, as in
`macos/arm64` or `linux/x64`; architectures are `arm64` (or `aarch64`) and
`x64` (or `x86_64`, `amd64`). A value without an architecture matches every
architecture on that platform.

## Tool dependencies {#tool-dependencies}

`depends` makes one tool wait for others to finish installing. Use it when a
tool's install or `postinstall` runs another tool that its backend does not
already require:

```toml [mise.toml]
[tools]
node = "24"
# postinstall runs npm, so node must finish installing first
# (github:example/acme is a placeholder)
"github:example/acme" = { version = "1.2.3", depends = ["node"], postinstall = "npm install --prefix ~/.acme acme-plugins" }
```

`depends` takes one tool name or a list. It only orders installation; it does
not add or install the listed tools. Each one must be in `[tools]`, where it
installs first, or already be on `PATH`. While the dependent tool installs and
runs its `postinstall`, the listed tools are on `PATH`. Plugin authors declare
a plugin's own requirements with `PLUGIN.depends`; see
[Tool plugin development](/tool-plugin-development.html#depends).

## Tool postinstall commands {#tool-postinstall-commands}

`postinstall` runs a command after this tool installs. It is separate from
the [`[hooks].postinstall`](/hooks.html) hook, which runs once after a whole
`mise install`:

```toml [mise.toml]
[tools]
node = { version = "24", postinstall = "corepack enable" }
```

By default the command runs only when mise installs or repairs the tool, never
after a failed install, and never with `--dry-run`. To run it on every
`mise install` and `mise use`, even when the version is already installed, set
`when = "always"`:

```toml [mise.toml]
[tools]
node = { version = "24", postinstall = { run = "corepack enable", when = "always" } }
```

The command runs with the tool's `bin` directory and any
[dependencies](#tool-dependencies) on `PATH`, the tool's `install_env`, and the
project's `[env]` values. An `[env]` value marked `tools = true` is included
only when the tool has install dependencies, from its backend (an `npm:` tool
depends on `node`) or from `depends`, and is resolved against those
dependencies. When `[env]` or one of those values cannot be resolved, the
command runs without it.
Templates such as <code v-pre>{{ tools.ripgrep.path }}</code> are rendered
first. It also receives:

- `MISE_TOOL_NAME`: the tool's short name, such as `node`.
- `MISE_TOOL_VERSION`: the version that was installed, such as `24.11.1`.
- `MISE_TOOL_INSTALL_PATH`: the directory the tool was installed to.
- `MISE_CONFIG_FILE`: the config file that declared the tool.
- `MISE_CONFIG_ROOT`: that file's [config root](/configuration.html#config-root).
- `MISE_PROJECT_ROOT`: the active project root, or the config root when no
  project is active.

A failing `postinstall` fails the install.

## Next steps {#next-steps}

- [Backends](/dev-tools/backends/) lists each backend, and each backend's page
  lists the options it reads.
- [Version requests and version files](/dev-tools/versions.html) covers what
  `version`, `prefix`, `ref`, and `path` accept.
- [Lazy tools](/dev-tools/shims.html#lazy-tools) covers `lazy` and `lazy_bins`.
- [Hooks](/hooks.html) run commands around a whole `mise install`, not one
  tool.
