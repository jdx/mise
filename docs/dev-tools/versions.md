---
description: "Write version requests such as 24, latest or prefix:1.20, and choose which version files mise reads."
---

# Version requests and version files

A version request is the value you give a tool in `mise.toml`, such as `"24"`
in `node = "24"`. mise resolves each request to a concrete version when it
runs; record the resolved versions in a [lockfile](/dev-tools/mise-lock.html)
when everyone must get the same build.

```toml [mise.toml]
[tools]
node = "24"          # a 24.x release
python = "3.14.8"    # exactly 3.14.8
ripgrep = "latest"   # the release the backend calls latest
```

## Request syntax {#request-syntax}

| Request                           | Example                            | Selects                                                                                                   |
| --------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Exact version                     | `"24.11.1"`                        | That release.                                                                                             |
| Prefix                            | `"24"`, `"3.14"`                   | A release that starts with the prefix. If a release is named exactly the prefix, that release is chosen.  |
| `latest`                          | `"latest"`                         | The release the tool's backend considers newest.                                                          |
| Alias                             | `"lts"`, `"project-lts"`           | The request an alias stands for, such as Node.js `lts` (`24`) or a [tool alias](/dev-tools/aliases.html). |
| `prefix:`                         | `"prefix:1.20"`                    | A release that starts with the prefix, even when a release has exactly that name.                         |
| `ref:`, `tag:`, `branch:`, `rev:` | `"ref:main"`                       | A build from a version-control reference, on backends that build from source.                             |
| `path:`                           | `"path:/opt/homebrew/opt/node@24"` | An existing directory, used as the installation.                                                          |
| `system`                          | `"system"`                         | No mise installation. mise adds nothing to `PATH` for the tool, so the copy already on `PATH` runs.       |
| `sub-`                            | `"sub-1:latest"`                   | The result of subtracting from another request's resolved version.                                        |

A prefix matches at separators: `1.2` matches `1.2.3` and `1.2+build`, but not
`1.20`. Prefixes and `latest` skip prereleases such as `1.0.0-rc1` unless you
request one exactly or set the `prerelease` [tool option](/dev-tools/#tool-options).

The same requests work on the command line (`mise use node@24`,
`mise exec go@prefix:1.20 -- go version`) and in `.tool-versions`. mise does not
accept npm-style ranges such as `^24` or `>=20` outside `package.json`: it warns
and then treats the range as a literal version, which fails to install.

`mise use node` without a version writes `latest`. `mise install node` and
`mise exec node` use the version the config selects, or `latest` when no config
declares the tool.

### Prefix, reference, path, and subtraction requests {#scopes}

`prefix:<PREFIX>` exists for tools that publish a release named like a prefix.
Go releases before 1.21 were named `1.20`, then `1.20.1`, so `go = "1.20"`
selects the `1.20` release itself. `go = "prefix:1.20"` selects the newest
1.20.x release instead.

`ref:<REF>` builds from a branch, tag, or commit. `tag:`, `branch:`, and `rev:`
name the kind of reference explicitly. Support depends on the backend: npm
packages from Git accept all four, cargo packages from Git accept `tag:`,
`branch:`, and `rev:` but not `ref:`, `spm` packages accept `ref:` and `rev:`,
and plugins accept the references they implement. The Python and Erlang core
tools do not build from references.

`path:<PATH>` uses a directory you built or installed elsewhere, such as a
Homebrew keg (`path:/opt/homebrew/opt/node@24`). `~/` expands to your home
directory, and a relative path resolves from the declaring config's
[config root](/configuration.html#config-root), or from the current directory on
the command line. On Windows, both separators work and mise stores the
forward-slash form. In TOML, a backslash starts an escape inside a
double-quoted string, so write a Windows path as a literal string
(`{ path = 'C:\tools\node' }`) or double the backslashes (`"C:\\tools\\node"`).
`"C:\tools\node"` is not rejected: TOML reads `\t` as a tab, so the path
silently changes. On Windows, mise rejects a path that contains a `cmd.exe`
metacharacter (`& | < > ^ %`), because the path is passed to plugins that build
shell commands with it.

`sub-<PARTIAL>:<REQUEST>` resolves `REQUEST`, subtracts `PARTIAL` from the
matching components of the result, and resolves what remains as a prefix.
`sub-2:lts` turns Node.js `24` into `22`, and `sub-0.1:latest` turns Python
`3.14.8` into a request for `3.13`. This is arithmetic on version numbers, not a
request for the Nth previous release.

In `mise.toml`, a table can name the request kind instead of a prefix:

```toml [mise.toml]
[tools]
go = { prefix = "1.20" }
"npm:github:owner/repo" = { ref = "main" }
node = { path = "/opt/homebrew/opt/node@24" }
```

## How a request resolves {#how-requests-resolve}

mise resolves a request in this order:

1. If a [lockfile](/dev-tools/mise-lock.html) has an entry for the request, mise
   uses the locked version.
2. Aliases expand, so `lts` becomes `24` for Node.js.
3. If an installed version matches the request, mise uses it. A newly published
   release does not change your environment until you install or upgrade.
4. Otherwise mise asks the tool's backend for its versions and picks a match.

Each backend decides what `latest` means. The github backend, for example, uses
the release GitHub marks as Latest, and most backends leave out prereleases.
Do not assume `latest` is the highest semantic version, or that a tool's
versions follow semver at all: tools use dates, channels such as `nightly`, and
names such as `lts-jod`.

`mise install node@24` resolves its argument against available releases, so it
installs the newest 24.x even when an older 24.x is installed. Other commands,
such as `mise use node@24` and `mise exec node@24`, reuse an installed match. A
`latest` request on the command line, such as
`mise exec node@latest -- node --version`, always checks for the newest
release.

To see what a request resolves to:

```sh
mise ls --current node          # the version this directory uses
mise latest --installed node@24 # the installed version that matches
mise latest node@24             # the newest available match
mise outdated                   # configured tools with newer matches
```

To move a project to newer releases, see [Upgrade tools](/dev-tools/#upgrade-tools).
Setting `MISE_NODE_VERSION=22` (or `MISE_<TOOL>_VERSION` for any tool)
overrides every config file for the commands that see it; see
[MISE\_\* variables](/configuration/environment-variables.html).

## Version ordering {#version-ordering}

Most backends list versions in the order their source returns them, and mise
picks the last match in that order. For aqua, github, gitlab, forgejo, and http
tools that publish releases out of order, such as a 1.x backport after 2.0, set
`version_order = "semver"` so that `latest`, prefixes, and `mise ls-remote` use
semantic version precedence:

```toml [mise.toml]
[tools]
"github:owner/tool" = { version = "latest", version_order = "semver" }
```

Versions that are not valid semver, such as `nightly`, keep their source order
ahead of the semantic versions and still match exactly. Build metadata does not
affect precedence. For `latest`, a release the backend marks as latest, such as
GitHub's Latest release, still wins unless it has no asset for this tool, which
happens in repositories that release several products.

Registry entries set `version_order` for their tools, so a shorthand such as
`ripgrep` may already use semver ordering. Set `version_order = "source"` to
restore the backend's order. The packslip backend always orders by semver.

## Multiple versions of a tool {#multiple-versions}

A tool can request several versions. mise installs all of them, and the first
one provides the unversioned command:

```toml [mise.toml]
[tools]
python = ["3.14", "3.13"]
```

```sh
mise exec -- python --version     # 3.14.x
mise exec -- python3.13 --version # 3.13.x
```

## Pin a version or use a lockfile {#pin-vs-lockfile}

A request such as `"24"` lets each machine use any 24.x release. To make every
machine use the same release, either pin or lock:

- `mise use --pin node@24` writes the resolved version, such as
  `node = "24.21.0"`, to `mise.toml`. The [`pin`](/configuration/settings.html#pin)
  setting makes this the default; `--fuzzy` overrides it.
- A [lockfile](/dev-tools/mise-lock.html) keeps `node = "24"` in `mise.toml`
  and records the resolved version, download URLs, and checksums in
  `mise.lock`. `mise upgrade` moves the lockfile within the request.

## `.tool-versions` {#tool-versions}

mise reads asdf's `.tool-versions` files the same way as `mise.toml`: from the
current directory and its parents. Use one when teammates still use asdf;
otherwise `mise.toml` supports options, environment variables, and tasks that
`.tool-versions` cannot express.

```text [.tool-versions]
node        24.11.1      # comments are allowed
ruby        3            # a prefix
shellcheck  latest
python      3.14 3.13    # several versions; the first is the default
go          prefix:1.20  # newest 1.20.x, not the release named 1.20
shfmt       path:./shfmt # use an existing directory
deno        sub-1:latest # one major version below the latest
```

asdf expects exact versions, so a shared file should hold the concrete
versions that `mise use --pin` writes. Keep prefixes such as `3`, `latest`,
`prefix:`, `sub-`, backend names such as `aqua:jqlang/jq`, and most aliases
out of a file that asdf users also read. When `mise.toml` and `.tool-versions`
in the same directory both declare a tool, `mise.toml` wins.
See [Migrating from asdf](/dev-tools/comparison-to-asdf.html) for sharing a file
with asdf users, and the
[asdf documentation](https://asdf-vm.com/manage/configuration.html#tool-versions)
for the file format.

## Idiomatic version files {#idiomatic-version-files}

Idiomatic version files are the version files other tools already use, such as
`.nvmrc` or `.python-version`. They let a project declare a version without
requiring mise. mise reads them only for tools you enable:

```sh
mise settings add idiomatic_version_file_enable_tools node
```

They accept the same aliases as the tool, so an `.nvmrc` that contains
`lts/hydrogen` works in both mise and nvm.

### Enable idiomatic version files {#enabling-idiomatic-version-files}

[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools)
lists the tools whose idiomatic files mise reads. It is empty by default; see
[discussion #4345](https://github.com/jdx/mise/discussions/4345) for why. To
stop reading them for a tool, for example because uv manages `.python-version`,
remove that tool from the list in your global config, or run
`mise settings unset idiomatic_version_file_enable_tools` to clear it.

To turn off one file while keeping the tool's others, add a `tool:filename`
pair to
[`idiomatic_version_file_disable_files`](/configuration/settings.html#idiomatic_version_file_disable_files).
This keeps `.nvmrc` for Node.js but leaves `package.json` to package managers:

```sh
mise settings add idiomatic_version_file_disable_files node:package.json
```

Finding and parsing these files has a small cost. Registry parsers run inside
mise, while plugin-provided files can run the plugin's parser, and the results
are [cached](/cache-behavior.html).

### Supported files {#supported-files}

<!-- mise:idiomatic-version-files:start -->

| Plugin        | Idiomatic Files                                                                                                                                                                                                                                                                                            |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| atmos         | `.atmos-version`                                                                                                                                                                                                                                                                                           |
| bazel         | `.bazelversion`                                                                                                                                                                                                                                                                                            |
| bun           | `.bun-version`, `package.json`                                                                                                                                                                                                                                                                             |
| chezmoi       | `.chezmoiversion`                                                                                                                                                                                                                                                                                          |
| cmake         | `CMakeLists.txt`                                                                                                                                                                                                                                                                                           |
| crystal       | `.crystal-version`                                                                                                                                                                                                                                                                                         |
| dagger        | `dagger.json`                                                                                                                                                                                                                                                                                              |
| deno          | `.deno-version`, `package.json`                                                                                                                                                                                                                                                                            |
| dotnet        | `global.json`                                                                                                                                                                                                                                                                                              |
| earthly       | `Earthfile`                                                                                                                                                                                                                                                                                                |
| elixir        | `.exenv-version`                                                                                                                                                                                                                                                                                           |
| go            | `.go-version`, `go.mod`, `go.work`                                                                                                                                                                                                                                                                         |
| golangci-lint | `.golangci.yml`, `.golangci.yaml`, `.golangci.toml`, `.golangci.json`                                                                                                                                                                                                                                      |
| goreleaser    | `.config/goreleaser.yml`, `.config/goreleaser.yaml`, `.goreleaser.yml`, `.goreleaser.yaml`, `goreleaser.yml`, `goreleaser.yaml`                                                                                                                                                                            |
| java          | `.java-version`, `.sdkmanrc`                                                                                                                                                                                                                                                                               |
| lefthook      | `lefthook.yml`, `lefthook.yaml`, `.lefthook.yml`, `.lefthook.yaml`, `lefthook.toml`, `.lefthook.toml`, `lefthook.json`, `.lefthook.json`, `lefthook.jsonc`, `.lefthook.jsonc`, `.config/lefthook.yml`, `.config/lefthook.yaml`, `.config/lefthook.toml`, `.config/lefthook.json`, `.config/lefthook.jsonc` |
| nim           | `.nim-version`                                                                                                                                                                                                                                                                                             |
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

asdf and vfox plugins can declare more files of their own. Registry entries can
describe how to extract a version from a structured file with the same
`version_regex`, `version_json_path`, and `version_expr` parsers as the
[HTTP backend](/dev-tools/backends/http.html), so tools installed through
backends such as `aqua:` and `github:` can read JSON manifests and other
tool-specific files without a plugin. For `.bazelversion` values, see the
[Bazel cookbook](/mise-cookbook/bazel.html).

asdf calls these files "legacy version files". mise calls them idiomatic
version files to separate an ecosystem's own conventions from mise's
configuration.

### Which fields mise reads {#which-fields-mise-reads}

mise reads only fields that declare the version a project is built with. It
ignores fields that declare a minimum compatible version, which is a floor for
the project's consumers. A library that still supports Node.js 18 or CMake 3.25
is almost certainly not developed with it, so resolving the floor would pin
everyone to the oldest supported release or, read as a range, to the newest.

A configuration-format major is different. A GoReleaser config's `version: 2`
selects a schema that is tied to the CLI major, so mise reads it and selects the
newest GoReleaser 2.x.

::: warning Deprecated minimum-version fields
mise used to read two floors as version requests: the `go X.Y` directive in
`go.mod` and `cmake_minimum_required` in `CMakeLists.txt`. Both are deprecated,
warn when they resolve a version, and stop being read in mise 2026.11.0. For
Go, add a `toolchain goX.Y.Z` line to `go.mod`, or use `.go-version` or
`mise.toml`. For CMake, use `mise.toml`.

To ignore the floors and silence the warning before then, set the
`idiomatic_version_file_ignore_minimum_versions` setting. That setting is
removed in 2026.11.0 along with the behavior it guards:

```sh
mise settings set idiomatic_version_file_ignore_minimum_versions true
```

:::

In `package.json`, mise reads development runtime and package-manager
declarations, not `engines` compatibility ranges. See the
[Node.js guide](/lang/node.html#package-json) for the fields, and the
[Bun](/lang/bun.html) and [Deno](/lang/deno.html) guides for theirs.

For Go, mise reads the `toolchain goX.Y.Z` directive from `go.mod` or
`go.work`, and an active workspace takes precedence over the modules beneath
it. See the [Go guide](/lang/go.html) for examples and `GOWORK` behavior.
