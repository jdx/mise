# Go

The following are instructions for using the go mise core plugin. This is used when there isn't a
git plugin installed named "go".

If you want to use [asdf-golang](https://github.com/kennyp/asdf-golang)
then use `mise plugins install go GIT_URL`.

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

## Settings

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable.

### `go_default_packages_file`

- Type: `string`
- Env: `MISE_GO_DEFAULT_PACKAGES_FILE`
- Default: `~/.default-go-packages`

Packages list to install with `go install` after installing a Go version.

### `go_download_mirror`

- Type: `string`
- Env: `MISE_GO_DOWNLOAD_MIRROR`
- Default: `https://dl.google.com/go`

URL to download go sdk tarballs from.

### `go_repo`

- Type: `string`
- Env: `MISE_GO_REPO`
- Default: `https://github.com/golang/go`

Used to get latest go version from GitHub releases.

### `go_set_gobin`

- Type: `bool | null`
- Env: `MISE_GO_SET_GOBIN`
- Default: `null`

Sets `GOBIN` to `~/.local/share/mise/go/installs/[VERSION]/bin`. This causes CLIs installed via
`go install` to have shims created for it which will have env vars from mise such as GOROOT set.

If not using shims or not using `go install` with tools that require GOROOT, it can probably be
safely disabled. See the [go backend](https://mise.jdx.dev/dev-tools/backends/) for the preferred
method to install Go CLIs.

### `go_set_gopath` <Badge type="warning" text="deprecated" />

- Type: `bool`
- Env: `MISE_GO_SET_GOPATH`
- Default: `false`

Sets `GOPATH` to `~/.local/share/mise/go/installs/[VERSION]/packages`. This retains behavior from
asdf and older mise versions. There is no known reason for this to be enabled but it is available
(for now) just in case anyone relies on it.

### `go_skip_checksum`

- Type: `bool`
- Env: `MISE_GO_SKIP_CHECKSUM`
- Default: `false`

Skips checksum verification of downloaded go tarballs.

## Default packages

mise can automatically install a default set of packages right after installing a new go version.
To enable this feature, provide a `$HOME/.default-go-packages` file that lists one packages per
line, for example:

```text
github.com/Dreamacro/clash # allows comments
github.com/jesseduffield/lazygit
```

## `.go-version` file support

mise uses a `mise.toml` or `.tool-versions` file for auto-switching between software versions.
However it can also read go-specific version files named `.go-version`.
