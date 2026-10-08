---
description: "Compile a Ruby command-line tool from GitHub into a native executable with Spinel."
---

# spinel backend <Badge type="warning" text="experimental" />

The `spinel` backend compiles a Ruby command-line tool from a GitHub repository
into a native executable with [Spinel](https://github.com/matz/spinel), Matz's
Ruby ahead-of-time compiler. The result runs without Ruby or Spinel installed.

::: warning Experimental
This backend may change, or be removed if Spinel or the backend stops being
practical to support. It installs tools without `experimental = true`, but
`mise edit` offers it only when that setting is on.
:::

## Requirements

- macOS or Linux; Windows is not supported
- `git`
- The `spinel` compiler on `PATH`, or its path in the [`spinel`](#spinel) tool
  option. Spinel ships as source; see its
  [Quick Start](https://github.com/matz/spinel#quick-start) to build it.
- A C compiler, which Spinel runs to produce the executable

Spinel supports only part of the Ruby language, so many Ruby tools do not
compile. The backend compiles a single entrypoint file. It does not install
gems, copy data files or fetch submodules.

## Usage

Most tools need options, such as the entrypoint, so declare them in `mise.toml`:

```toml
[tools."spinel:tobi/try"]
version = "1.10.1"
entrypoint = "try.rb"
bin = "try"
tag_prefix = "v"
```

Then install the tool:

```sh
mise install
```

mise fetches the tag `v1.10.1`, compiles `try.rb` and writes the executable to
`bin/try` in the install directory.

`mise ls-remote spinel:tobi/try` lists the repository's Git tags with
`git ls-remote`, so it needs no GitHub token. With `tag_prefix = "v"`, the tag
`v1.10.1` is the version `1.10.1`.

mise keeps the order that `git ls-remote` returns, which is alphabetical, and
this backend does not support `version_order`. `latest` can therefore pick an
older release when versions differ in the number of digits, such as `1.9.3`
over `1.10.1`. Pin a version.

## Tool options

### `entrypoint`

The Ruby file to compile, relative to the repository root. Defaults to
`main.rb`.

### `bin`

The name of the executable written to `bin/`. Defaults to the repository name.

### `tag_prefix` {#tag-prefix}

The text before the version in each Git tag, such as `v`. mise lists only tags
that start with the prefix and removes it. Without it, every tag is listed as a
version.

### `source_ref` {#source-ref}

A full 40-character commit SHA to build instead of the tag. The version is then
only a label.

### `spinel`

The compiler to run, as a command name or a path. Defaults to `spinel` on
`PATH`.

## Troubleshooting

| Message                                       | What to check                                                                                  |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `spinel may be required but was not found`    | Build Spinel and put it on `PATH`, or set the [`spinel`](#spinel) option.                      |
| `spinel did not produce an executable`        | The compiler output above the error; the entrypoint may use Ruby that Spinel does not support. |
| `the spinel backend does not support Windows` | Install the tool on macOS or Linux.                                                            |

Implementation: [`src/backend/spinel.rs`](https://github.com/jdx/mise/blob/main/src/backend/spinel.rs).
