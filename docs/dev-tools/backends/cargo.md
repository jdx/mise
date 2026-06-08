# Cargo Backend

You may install packages directly from [Cargo Crates](https://crates.io/) even if there
isn't an asdf plugin for it.

The code for this is inside the mise repository at [`./src/backend/cargo.rs`](https://github.com/jdx/mise/blob/main/src/backend/cargo.rs).

## Dependencies

This relies on having `cargo` installed. You can either install it on your
system via [rustup](https://rustup.rs/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Or you can install it via mise:

```sh
mise use -g rust
```

## Usage

The following installs the latest version of [eza](https://crates.io/crates/eza) and
sets it as the active version on PATH:

```sh
$ mise use -g cargo:eza
$ eza --version
eza - A modern, maintained replacement for ls
v0.17.1 [+git]
https://github.com/eza-community/eza
```

The version will be set in `~/.config/mise/config.toml` with the following format:

```toml
[tools]
"cargo:eza" = "latest"
```

### Using Git

You can install any package from a Git repository using the `mise` command. This allows you to
install a particular tag, branch, or commit revision:

```sh
# Install a specific tag
mise use cargo:https://github.com/username/demo@tag:<release_tag>

# Install the latest from a branch
mise use cargo:https://github.com/username/demo@branch:<branch_name>

# Install a specific commit revision
mise use cargo:https://github.com/username/demo@rev:<commit_hash>
```

This will execute a `cargo install` command with the corresponding Git options.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

Some Cargo settings are only meaningful when mise runs `cargo install`. If `cargo-binstall`
installs a prebuilt binary, Cargo build settings and `cargo install` behavior do not affect that
artifact. Set `cargo.binstall = false` when you need Cargo settings to control the install.

When mise uses `cargo-binstall`, mise runs `cargo-binstall` once and lets `cargo-binstall` handle
its own fallback order, including its final fallback to compiling with `cargo install`. mise does
not retry with a separate `cargo install` command if `cargo-binstall` exits with an error.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="cargo" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `cargo` backend—these
go in `[tools]` in `mise.toml`.

When `cargo-binstall` is available, mise uses it for registry installs unless a tool option needs
`cargo install` to build from source.

For options that do not skip `cargo-binstall`, any source-build fallback is handled by
`cargo-binstall` itself. mise does not perform an additional compile fallback after
`cargo-binstall` fails.

| Option                     | `cargo-binstall` behavior                                                                |
| -------------------------- | ---------------------------------------------------------------------------------------- |
| `features`                 | Skips `cargo-binstall`; requires `cargo install --features`.                             |
| `default-features = false` | Skips `cargo-binstall`; requires `cargo install --no-default-features`.                  |
| `bin`                      | Passed through to `cargo-binstall`; does not skip it.                                    |
| `crate`                    | Does not skip `cargo-binstall` when applicable. Git installs always use `cargo install`. |
| `locked`                   | Passed through to `cargo-binstall`; does not skip it.                                    |

### `install_env`

Set environment variables for the `cargo install` or `cargo-binstall` command:

```toml
[tools]
"cargo:eza" = { version = "latest", install_env = { CARGO_NET_GIT_FETCH_WITH_CLI = "true" } }
```

### `features`

Install additional components (passed as `cargo install --features`):

```toml
[tools]
"cargo:cargo-edit" = { version = "latest", features = "add" }
```

This option requires `cargo install`; mise skips `cargo-binstall` when it is set.

### `default-features`

Disable default features (passed as `cargo install --no-default-features`):

```toml
[tools]
"cargo:cargo-edit" = { version = "latest", default-features = false }
```

Setting this to `false` requires `cargo install`; mise skips `cargo-binstall` in that case.

### `bin`

Select the CLI bin name to install when multiple are available (passed as `cargo install --bin`):

```toml
[tools]
"cargo:https://github.com/username/demo" = { version = "tag:v1.0.0", bin = "demo" }
```

This option is supported by `cargo-binstall`, so it does not cause mise to skip `cargo-binstall`.

### `crate`

Select the crate name to install when multiple are available (passed as
`cargo install --git=<repo> <crate>`):

```toml
[tools]
"cargo:https://github.com/username/demo" = { version = "tag:v1.0.0", crate = "demo" }
```

This option does not cause mise to skip `cargo-binstall` when applicable. Git installs already use
`cargo install`.

### `locked`

Use Cargo.lock (passes `cargo install --locked`) when building CLI. This is the default behavior,
pass `false` to disable:

```toml
[tools]
"cargo:https://github.com/username/demo" = { version = "latest", locked = false }
```

This option does not cause mise to skip `cargo-binstall`; it only affects the install if
`cargo-binstall` itself falls back to compiling with `cargo install`.
