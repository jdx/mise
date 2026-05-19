# Go

`mise` can be used to install and manage multiple versions of [go](https://golang.org/) on the same system.

> The following are instructions for using the go mise core plugin. This is used when there isn't a
> git plugin installed named "go". If you want to use [asdf-golang](https://github.com/kennyp/asdf-golang)
> then use `mise plugins install go GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/go.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/go.rs).

## Usage

The following installs the latest version of go-1.21.x (if some version of 1.21.x is not already
installed) and makes it the global default:

```sh
mise use -g go@1.21
```

Minor go versions 1.20 and below require specifying `prefix` before the version number because the
first version of each series was released without a `.0` suffix, making 1.20 an exact version match:

```sh
mise use -g go@prefix:1.20
```

## `.go-version` file support

mise uses a `mise.toml` or `.tool-versions` file for auto-switching between software versions.
However, it can also read go-specific version files named `.go-version`.

See [idiomatic version files](/configuration.html#idiomatic-version-files)

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
