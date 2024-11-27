# Ubi Backend

You may install GitHub Releases and URL packages directly using [ubi](https://github.com/houseabsolute/ubi) backend. ubi is directly compiled into
the mise codebase so it does not need to be installed separately to be used. ubi is preferred over
asdf/vfox for new tools since it doesn't require a plugin, supports Windows, and is really easy to use.

ubi doesn't require plugins or even any configuration for each tool. What it does is try to deduce what
the proper binary/tarball is from GitHub releases and downloads the right one. As long as the vendor
uses a somewhat standard labeling scheme for their releases, ubi should be able to figure it out.

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

- **GitHub shorthand for latest release version:** `ubi:goreleaser/goreleaser`
- **GitHub shorthand for specific release version:** `ubi:goreleaser/goreleaser@1.25.1`
- **URL syntax:** `ubi:https://github.com/goreleaser/goreleaser/releases/download/v1.16.2/goreleaser_Darwin_arm64.tar.gz`

## Troubleshooting ubi

### `ubi` resolver can't find os/arch

Sometimes vendors use strange formats for their releases that ubi can't figure out, possibly for a
specific os/arch combination. For example this recently happend in [this ticket](https://github.com/houseabsolute/ubi/issues/79) because a vendor used
"mac" instead of the more common "macos" or "darwin" tags.

Try using ubi by itself to see if the issue is related to mise or ubi:

```sh
ubi -p jdx/mise
./bin/mise -v # yes this technically means you could do `mise use ubi:jdx/mise` though I don't know why you would
```

### `ubi` picks the wrong tarball

Another issue is that a GitHub release may have a bunch of tarballs, some that don't contain the CLI
you want, you can use the `matching` field in order to specify a string to match against the release.

```sh
mise use ubi:tamasfe/taplo[matching=full]
# or with ubi directly
ubi -p tamasfe/taplo -m full
```

### `ubi` can't find the binary in the tarball

ubi assumes that the repo name is the same as the binary name, however that is often not the case.
For example, BurntSushi/ripgrep gives us a binary named `rg` not `ripgrep`. In this case, you can
specify the binary name with the `exe` field:

```sh
mise use ubi:BurntSushi/ripgrep[exe=rg]
# or with ubi directly
ubi -p BurntSushi/ripgrep -e rg
```

### `ubi` uses weird versions

This issue is actually with mise and not with ubi. mise needs to be able to list the available versions
of the tools so that "latest" points to whatever is the actual latest release of the CLI. What sometimes
happens is vendors will have GitHub releases for unrelated things. For example, `cargo-bins/cargo-binstall`
is the repo for cargo-binstall, however it has a bunch of releases for unrelated CLIs that are not
cargo-binstall. We need to filter these out and that can be specified with the `tag_regex` tool option:

```sh
mise use 'ubi:cargo-bins/cargo-binstall[tag_regex=^\d+\.]'
```

Now when running `mise ls-remote ubi:cargo-bins/cargo-binstall[tag_regex=^\d+\.]` you should only see
versions starting with a number. Note that this command is cached so you likely will need to run `mise cache clear` first.
