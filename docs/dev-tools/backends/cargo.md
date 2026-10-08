---
description: "Install Rust command-line tools from crates.io or Git, as prebuilt binaries or built with Cargo."
---

# cargo backend

The `cargo` backend installs Rust command-line tools from
[crates.io](https://crates.io/) or a Git repository. It downloads a prebuilt
binary when cargo-binstall can find one and builds the crate with Cargo
otherwise. It is for executables; add library crates to your project with
`cargo add`.

## Requirements

<span id="dependencies"></span>

Install Rust so mise can run `cargo install`. To skip compiling for crates that
publish prebuilt binaries, add cargo-binstall too:

```sh
mise use -g rust cargo-binstall
```

When `rust`, `cargo-binstall` or `sccache` is in your config, mise installs it
before your Cargo tools. Source builds also need a linker and any native
libraries the crate uses.

## Usage

Declare Rust and eza together in the current project:

```sh
mise use rust@stable cargo:eza
mise exec -- eza --version
```

This records both tools in `mise.toml`. Add `-g` to `mise use` for your global
config.

```toml
[tools]
rust = "stable"
"cargo:eza" = "latest"
```

Run `mise ls-remote cargo:eza` to list releases, and pin one with
`mise use cargo:eza@0.23.5`.

### Install from Git {#using-git}

Give a Git repository URL, or `owner/repo` for GitHub, and pick a tag, branch
or commit as the version. Quote the argument:

```sh
mise use 'cargo:https://github.com/eza-community/eza@tag:v0.23.5'
mise use 'cargo:eza-community/eza@branch:main'
mise use 'cargo:eza-community/eza@rev:<commit>'
```

mise runs `cargo install --git` with the matching `--tag`, `--branch` or
`--rev`. A Git source lists only `HEAD` as a version, so `latest`, or no
version at all, installs the default branch.

## How mise installs a crate

mise uses the first method that applies:

1. A Git source always builds with `cargo install --git`.
2. The `features` option, or `default-features = false`, needs a source build,
   so mise runs `cargo install`.
3. If [`cargo.binstall`](/dev-tools/backends/cargo.html#cargo.binstall) is on (the default) and
   `cargo-binstall` is installed through mise or on `PATH`, mise runs
   cargo-binstall with its compile strategy turned off. When cargo-binstall
   reports that no prebuilt binary exists (exit code 94), mise runs
   `cargo install` instead. Other cargo-binstall errors fail the install.
4. If cargo-binstall is not installed and
   [`cargo.binstall_native = true`](/dev-tools/backends/cargo.html#cargo.binstall_native), mise tries its own
   prebuilt-binary installer and runs `cargo install` when it finds no binary.
5. Otherwise mise runs `cargo install`.

Set [`cargo.binstall_only = true`](/dev-tools/backends/cargo.html#cargo.binstall_only) to fail instead of
building from source; options that need a source build then fail too. Set
`cargo.binstall = false` to always build. A prebuilt binary ignores your Cargo
build configuration, so turn binstall off when that configuration must apply.

By default, mise also stops cargo-binstall from using the third-party
[cargo-quickinstall](https://github.com/cargo-bins/cargo-quickinstall) artifact
host, so it uses only artifacts published by the crate's authors. Set
[`cargo.binstall_quickinstall = true`](/dev-tools/backends/cargo.html#cargo.binstall_quickinstall) to allow
it. mise's own installer never uses quickinstall.

## Tool options

Set these on the tool's entry in `[tools]`, or inline, as in
`'cargo:eza[locked=false]'`. Options every backend accepts are described under
[tool options](/dev-tools/#tool-options).

mise records the `features`, `default-features`, `bin`, `crate` and `locked`
values with each installed version. Changing one reinstalls that version instead
of reusing a binary built with other options. Feature names are normalized, so
reordering them or switching between a string and an array does not trigger a
reinstall.

| Option                     | Passed as               | Skips cargo-binstall |
| -------------------------- | ----------------------- | -------------------- |
| `features`                 | `--features`            | Yes                  |
| `default-features = false` | `--no-default-features` | Yes                  |
| `bin`                      | `--bin`                 | No                   |
| `crate`                    | the crate name          | No                   |
| `locked`                   | `--locked`              | No                   |

### `install_env`

Set environment variables for `cargo install` or `cargo-binstall`:

```toml
[tools]
"cargo:eza" = { version = "latest", install_env = { CARGO_NET_GIT_FETCH_WITH_CLI = "true" } }
```

### `features`

Enable crate features:

```toml
[tools]
"cargo:sqlx-cli" = { version = "latest", features = ["postgres", "rustls"] }
```

### `default-features`

Turn off the crate's default features:

```toml
[tools]
"cargo:cargo-edit" = { version = "latest", default-features = false }
```

### `bin`

Install one executable from a crate that has several:

```toml
[tools]
"cargo:sqlx-cli" = { version = "latest", bin = "sqlx" }
```

### `crate`

Choose the crate to install from a Git repository that contains several, such
as a Cargo workspace:

```toml
[tools]
"cargo:https://github.com/astral-sh/ruff" = { version = "tag:0.6.0", crate = "ruff" }
```

### `locked`

Build with the crate's own `Cargo.lock` (`cargo install --locked`). This is the
default; set `false` to let Cargo resolve dependencies again:

```toml
[tools]
"cargo:eza" = { version = "latest", locked = false }
```

This is the crate's `Cargo.lock`, not [`mise.lock`](/dev-tools/mise-lock.html),
which records the version of the tool mise installed.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="cargo" :level="3" />

## Troubleshooting

| Symptom                                                         | What to do                                                                                                                                 |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Compiler or linker error                                        | Read the first Cargo error and install the crate's native build dependencies. `features` always builds from source.                        |
| No executable installed                                         | The crate must have a binary target. Use `bin`, or `crate` for a Git workspace.                                                            |
| A prebuilt binary ignores your Cargo config                     | Set `cargo.binstall = false` to build locally.                                                                                             |
| `cargo-binstall cannot honor cargo install-only tool option(s)` | `cargo.binstall_only` is set and the tool uses `features` or `default-features = false`. Remove the option or unset `cargo.binstall_only`. |

Implementation: [`src/backend/cargo.rs`](https://github.com/jdx/mise/blob/main/src/backend/cargo.rs).
