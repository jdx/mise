---
description: "Compile Ruby command-line tools from GitHub source with the experimental Spinel backend."
---

# Spinel Backend <Badge type="warning" text="experimental" />

The `spinel:` backend compiles a Ruby command-line tool from a GitHub repository into a native
executable with [Spinel](https://github.com/matz/spinel), Matz's Ruby AOT compiler. The result needs
neither Ruby nor Spinel to run.

::: warning
This backend is experimental, and it may not stay. Spinel is young, and the backend is only worth
keeping while Spinel takes off and the backend stays cheap to maintain. If either stops being true,
it will be removed. Enable it with `mise settings experimental=true`.
:::

## Requirements

- macOS or Linux (Windows is not supported).
- `git`.
- The `spinel` compiler on `PATH`, or its path in the `spinel` tool option. Spinel ships as source:
  see its [Quick Start](https://github.com/matz/spinel#quick-start) to build it.
- A C compiler, which Spinel invokes to produce the executable.

Spinel supports only part of the Ruby language, so many Ruby tools will not compile. The backend
compiles a single entrypoint file. It does not install gems, copy data files, or fetch submodules.

## Usage

```sh
mise settings experimental=true
mise use spinel:tobi/try@1.10.1
```

```toml
[tools."spinel:tobi/try"]
version = "1.10.1"
entrypoint = "try.rb"
bin = "try"
tag_prefix = "v"
```

`mise ls-remote spinel:tobi/try` lists the repository's git tags (via `git ls-remote`, not the GitHub
API). With `tag_prefix = "v"`, the tag `v1.10.1` is the version `1.10.1`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `spinel` backend—these
go in `[tools]` in `mise.toml`.

### `entrypoint`

The Ruby file to compile, relative to the repository root. Defaults to `main.rb`.

### `bin`

The name of the executable written to `bin/`. Defaults to the repository name.

### `tag_prefix`

Text before the version in each git tag, such as `v`. Only tags with the prefix are listed.

### `source_ref`

A full 40-character commit SHA to build instead of the tag. The version is then only a label.

### `spinel`

The compiler to run. Defaults to `spinel` on `PATH`.

Implementation: [`src/backend/spinel.rs`](https://github.com/jdx/mise/blob/main/src/backend/spinel.rs).
