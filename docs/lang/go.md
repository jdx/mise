---
description: "Install Go with mise, read toolchain lines from go.mod and go.work, and manage GOROOT and GOBIN."
---

# Go

mise installs the official [Go](https://go.dev/) distributions and sets
`GOROOT` and `GOBIN` for the selected version.

## Quick start

Select a Go release series for the current project:

```sh
mise use go@1.27
mise exec -- go version
```

Use `mise use -g go@1.27` for a personal default. [`mise upgrade go`](/cli/upgrade.html)
updates within the configured request.

## Choosing a version

| Request          | Selects                                |
| ---------------- | -------------------------------------- |
| `go@1.27`        | The newest 1.27.x release              |
| `go@1.27.1`      | That release                           |
| `go@latest`      | The newest release                     |
| `go@1.20`        | Exactly `1.20`, the first 1.20 release |
| `go@prefix:1.20` | The newest 1.20.x release              |

Go 1.20 and earlier published the first release of each series as `1.20`, not
`1.20.0`, so `go@1.20` selects that exact first release. To get the newest
1.20.x, use `prefix:`:

```sh
mise use go@prefix:1.20
```

From Go 1.21 on, the first release is `1.21.0`, so `go@1.21` already selects
the newest 1.21.x. Betas and release candidates are not listed. Run
`mise ls-remote go` to see every version.

## Version files {#go-version-file-support}

mise can select Go from `.go-version`, `go.mod` or `go.work`. Enable them for
Go:

```sh
mise settings add idiomatic_version_file_enable_tools go
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

In `go.mod` and `go.work`, mise reads a `toolchain goX.Y.Z` directive as an
exact version request. For example, this `go.work` selects Go 1.27.1 for a
workspace containing the `api` and `worker` modules:

```text [go.work]
go 1.27.0

toolchain go1.27.1

use (
    ./api
    ./worker
)
```

The `go` directive only declares the minimum Go version that the module
supports. mise ignores it in `go.work`. Reading it from `go.mod` is deprecated
and stops in mise 2026.11.0; add a `toolchain` line instead, or set
`idiomatic_version_file_ignore_minimum_versions` to ignore it now. See
[which fields mise reads](/dev-tools/versions.html#which-fields-mise-reads).

### Workspaces and `GOWORK`

When a workspace is active, mise uses its `go.work` instead of the `go.mod`
files beneath it, including when you run mise from a module subdirectory. If
the workspace has no supported `toolchain` directive, neither file supplies a
version. Use `toolchain goX.Y.Z` with a full release version; mise does not
read `toolchain default`, partial versions or release candidates.

The `GOWORK` environment variable controls which workspace mise uses:

| `GOWORK` value          | mise behavior                                                                                                           |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Unset, empty, or `auto` | Search the current directory and its parents for `go.work`, within mise's config search paths.                          |
| `off`                   | Ignore `go.work` and read `go.mod`.                                                                                     |
| Absolute path           | Use only the named workspace file, if mise discovers it; ignore other `go.work` files and do not fall back to `go.mod`. |

Setting `GOWORK` does not make mise read files outside its config search paths.
If the named workspace is outside those paths or does not exist, it supplies no
version; set the Go version in `mise.toml` for such a workspace. mise ignores a
relative `GOWORK` path and uses its default search. Go itself rejects relative
paths, so use an absolute path.

### Go's own toolchain switching {#go-toolchain-selection}

Go has its own [toolchain selection](https://go.dev/doc/toolchain), controlled
by `GOTOOLCHAIN` and the `go` and `toolchain` lines in `go.mod` and `go.work`.
With Go's default `GOTOOLCHAIN=auto`, the `go` command that mise installed can
download and run a newer toolchain when a module asks for one. Compare
`mise exec -- go version` with `mise exec -- go env GOTOOLCHAIN` when the
version you see is not the one mise selected. To make Go always run the
toolchain mise selected, set `GOTOOLCHAIN` in `[env]`:

```toml [mise.toml]
[env]
GOTOOLCHAIN = "local"
```

## Binaries from `go install`

Unless `GOBIN` is already set in your shell, mise sets it to the selected Go
version's `bin` directory, which is on `PATH`. A command installed with
`go install` therefore belongs to that Go version and is gone after you switch
or upgrade Go.

To keep a Go CLI across Go versions, install it as its own tool with the
[`go:` backend](/dev-tools/backends/go.html), for example
`mise use -g go:golang.org/x/tools/gopls`. To use Go's default location
(`$GOPATH/bin`, or `~/go/bin`), set [`go.set_gobin`](/lang/go.html#go.set_gobin) to `false`
and add that directory to your `PATH` yourself.

## Environment variables

| Variable | Value                                                                                                   |
| -------- | ------------------------------------------------------------------------------------------------------- |
| `GOROOT` | The selected version's install directory. Turn off with [`go.set_goroot`](/lang/go.html#go.set_goroot). |
| `GOBIN`  | The selected version's `bin` directory, unless `GOBIN` is already set or `go.set_gobin` is `false`.     |

mise does not set `GOPATH`. Set it in `[env]` if you need a specific value.

## How mise installs Go

mise lists Go versions from its
[versions host](/configuration/settings.html#use_versions_host), or from the
release tags of [`go.repo`](/lang/go.html#go.repo) when `use_versions_host` is off. mise
downloads the archive for your platform from
[`go.download_mirror`](/lang/go.html#go.download_mirror) (`https://dl.google.com/go` by
default), checks it against the `.sha256` file published next to it, and runs
`go version`. [`go.skip_checksum`](/lang/go.html#go.skip_checksum) turns the checksum check
off.

An installed plugin named `go` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

Go has no Go-specific options. `install_env` reaches the `go version` check,
`go install` for default packages, and `postinstall` commands. For example, a
`postinstall` that installs a private module needs `GOPRIVATE`:

```toml [mise.toml]
[tools]
go = {
  version = "1.27",
  install_env = { GOPRIVATE = "github.com/acme/*" },
  postinstall = "go install github.com/acme/tool@latest",
}
```

Other generic options are described in [tool options](/dev-tools/#tool-options).

## Default packages file <Badge type="danger" text="deprecated" /> {#default-packages}

mise installs the packages listed in `~/.default-go-packages`
([`go.default_packages_file`](/lang/go.html#go.default_packages_file)), one per line, with
`go install` into each new Go version. mise warns about this file from 2026.11.0
and stops reading it in 2027.11.0. Install Go CLIs with the
[`go:` backend](/dev-tools/backends/go.html) instead, for example
`"go:github.com/jesseduffield/lazygit" = "latest"`, or use a
[`postinstall`](/dev-tools/#tool-postinstall-commands) command for packages
every Go version needs.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="go" :level="3" />
