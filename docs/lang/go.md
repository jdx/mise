# Go in mise

The following are instructions for using the go mise core plugin. This is used when there isn't a
git plugin installed named "go".

If you want to use [asdf-golang](https://github.com/kennyp/asdf-golang)
or [mise-golang](https://github.com/rtx-plugins/mise-golang)
then use `mise plugins install go GIT_URL`.

The code for this is inside the mise repository at
[`./src/plugins/core/go.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/go.rs).

## Usage

The following installs the latest version of go-1.20.x (if some version of 1.20.x is not already
installed) and makes it the global default:

```sh
mise use -g go@1.20
```

## Configuration

- `MISE_GO_SKIP_CHECKSUM` [bool]: skips checksum verification of downloaded go tarballs, defaults to false
- `MISE_GO_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-go-packages`
- `MISE_GO_DOWNLOAD_MIRROR` [string]: location to download go from, defaults to `https://dl.google.com/go`
- `MISE_GO_SET_GOROOT` [bool]: set `$GOROOT` to the mise go installs go root dir, defaults to true
- `MISE_GO_SET_GOPATH` [bool]: set `$GOPATH` to the mise go installs packages dir, defaults to true

## Default packages

mise can automatically install a default set of packages right after installing a new go version.
To enable this feature, provide a `$HOME/.default-go-packages` file that lists one packages per
line, for example:

```text
github.com/Dreamacro/clash # allows comments
github.com/jesseduffield/lazygit
```

## `.go-version` file support

mise uses a `.tool-versions` or `.mise.toml` file for auto-switching between software versions.
However it can also read go-specific version files named `.go-version`.
