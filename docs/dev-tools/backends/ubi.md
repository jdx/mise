# Ubi Backend

You may install GitHub Releases and URL packages directly using [ubi](https://github.com/houseabsolute/ubi) backend. ubi is directly compiled into
the mise codebase so it does not need to be installed separately to be used. ubi is the preferred backend when it functions for tools as it is the
simplest and requires minimal configuration.

The code for this is inside of the mise repository at [`./src/backend/ubi.rs`](https://github.com/jdx/mise/blob/main/src/backend/ubi.rs).

## Usage

The following installs the latest version of goreleaser
and sets it as the active version on PATH:

```sh
$ mise use -g ubi:goreleaser/goreleaser
$ goreleaser --version
1.25.1
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"ubi:goreleaser/goreleaser" = "latest"
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `ubi` backend—these
go in `[tools]` in `mise.toml`.

### `exe`

The `exe` option allows you to specify the executable name in the archive. This is useful when the
archive contains multiple executables.

If you get an error like `could not find any files named cli in the downloaded zip file`, you can
use the `exe` option to specify the executable name:

```toml
[tools]
"ubi:cli/cli" = { exe = "gh" } # github's cli
```

### `matching`

Set a string to match against the release filename when there are multiple files for your
OS/arch, i.e. "gnu" or "musl". Note that this is only used when there is more than one
matching release filename for your OS/arch. If only one release asset matches your OS/arch,
then this will be ignored.

```toml
[tools]
"ubi:BurntSushi/ripgrep" = { matching = "musl" }
```

## Supported Ubi Syntax

| Description                                   | Usage                                                                                                   |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| GitHub shorthand for latest release version   | `ubi:goreleaser/goreleaser`                                                                             |
| GitHub shorthand for specific release version | `ubi:goreleaser/goreleaser@1.25.1`                                                                      |
| URL syntax                                    | `ubi:https://github.com/goreleaser/goreleaser/releases/download/v1.16.2/goreleaser_Darwin_arm64.tar.gz` |

Other syntax may work but is unsupported and untested.
