# Go in rtx

The following are instructions for using the go rtx core plugin. This is used when there isn't a 
git plugin installed named "go".

If you want to use [asdf-golang](https://github.com/kennyp/asdf-golang)
or [rtx-golang](https://github.com/rtx-plugins/rtx-golang)
then use `rtx plugins install go GIT_URL`.

The code for this is inside the rtx repository at
[`./src/plugins/core/go.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/go.rs).

## Usage

The following installs the latest version of go-1.20.x (if some version of 1.20.x is not already
installed) and makes it the global default:

```sh-session
$ rtx use -g go@1.20
```

## Configuration

- `RTX_GO_SKIP_CHECKSUM` [bool]: skips checksum verification of downloaded go tarballs, defaults to false
- `RTX_GO_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-go-packages`
- `RTX_GO_DOWNLOAD_MIRROR` [string]: location to download go from, defaults to `https://dl.google.com/go`
- `RTX_GO_SET_GOROOT` [bool]: set `$GOROOT` to the rtx go installs go root dir, defaults to true
- `RTX_GO_SET_GOPATH` [bool]: set `$GOPATH` to the rtx go installs packages dir, defaults to true

## Default packages

rtx can automatically install a default set of packages right after installing a new go version. 
To enable this feature, provide a `$HOME/.default-go-packages` file that lists one packages per 
line, for example:

```
github.com/Dreamacro/clash # allows comments
github.com/jesseduffield/lazygit
```

## `.go-version` file support

rtx uses a `.tool-versions` or `.rtx.toml` file for auto-switching between software versions.
However it can also read go-specific version files named `.go-version`.
