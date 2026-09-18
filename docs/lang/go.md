---
description: "mise can be used to install and manage multiple versions of go on the same system."
---

# Go

`mise` can be used to install and manage multiple versions of [go](https://golang.org/) on the same system.

## Usage

Select a Go release series for the current project:

```sh
mise use go@1.25
mise exec -- go version
```

Use `mise use -g go@1.25` for a personal default. `mise ls-remote go` lists
available versions; `mise upgrade go` updates within the configured request.

Minor go versions 1.20 and below require specifying `prefix` before the version number because the
first version of each series was released without a `.0` suffix, making 1.20 an exact version match:

```sh
mise use go@prefix:1.20
```

These instructions use mise's built-in go support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/go.rs)
for backend details.

## Go version files {#go-version-file-support}

Enable Go idiomatic version files to select a version from `.go-version`, `go.mod`, or `go.work`:

```sh
mise settings add idiomatic_version_file_enable_tools go
```

For `go.mod` and `go.work`, mise reads a `toolchain goX.Y.Z` directive as an exact version
request. For example, this `go.work` selects Go 1.24.3 for a workspace containing the `api` and
`worker` modules:

```text
go 1.24.0

toolchain go1.24.3

use (
    ./api
    ./worker
)
```

The `go` directive declares a minimum required version. mise ignores it in `go.work`; reading it
from `go.mod` is deprecated (see [Which fields mise reads](/configuration.html#which-fields-mise-reads)).

### Workspaces and `GOWORK`

When a workspace is active, mise uses its `go.work` instead of the `go.mod` files beneath it,
including when you run mise from a module subdirectory. If the workspace has no supported
`toolchain` directive, neither file supplies a version. Use `toolchain goX.Y.Z` with a full
release version; mise does not read `toolchain default`, partial versions, or release candidates.

The `GOWORK` environment variable controls workspace selection:

| `GOWORK` value          | mise behavior                                                                                                           |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Unset, empty, or `auto` | Search the current directory and its parents for `go.work`, within mise's configuration search paths.                   |
| `off`                   | Ignore `go.work` and read `go.mod`.                                                                                     |
| Absolute path           | Use only the named workspace file, if mise discovers it; ignore other `go.work` files and do not fall back to `go.mod`. |

::: tip Workspace discovery limits
Setting `GOWORK` does not make mise read files outside its configuration search paths. If the
named workspace is outside those paths or does not exist, it supplies no version. Set the Go
version in `mise.toml` for a workspace outside those paths.

mise ignores a relative `GOWORK` path and uses its default workspace search. Go rejects relative
paths, so use an absolute path when setting `GOWORK`.
:::

### Go toolchain selection

Go also has its own [toolchain selection](https://go.dev/doc/toolchain), controlled
by `GOTOOLCHAIN` and module/workspace declarations. It may use a different
toolchain after mise starts it. Compare `mise exec -- go version` with
`mise exec -- go env GOTOOLCHAIN` when investigating unexpected versions.

## Default packages

::: warning Planned deprecation
Default package files are deprecated. They are still supported for now, but mise will start warning
in `2026.11.0` and support will be removed in `2027.11.0`.

For Go CLIs, install the tool directly with the `go:` backend:

```toml
[tools]
"go:github.com/jesseduffield/lazygit" = "latest"
```

For packages that really should be installed into every Go version, use a tool-level `postinstall`
hook:

```toml
[tools]
go = { version = "1.25", postinstall = "go install github.com/daixiang0/gci@latest" }
```

:::

mise can automatically install a default set of packages right after installing a new go version.
To use this legacy feature, provide a `$HOME/.default-go-packages` file that lists one package per
line, for example:

```text
github.com/daixiang0/gci # allows comments
github.com/jesseduffield/lazygit
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `go` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for default package installation and install-time verification commands
run by the core `go` backend:

```toml
[tools]
go = { version = "latest", install_env = { GOPRIVATE = "github.com/acme/*" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="go" :level="3" />
