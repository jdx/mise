# Cargo Backend

The `cargo` backend installs Rust command-line applications from [crates.io](https://crates.io/)
or a Git repository. It can use published binaries through cargo-binstall or build
the crate with Cargo. Application dependencies belong in your `Cargo.toml`.

The code for this is inside the mise repository at [`./src/backend/cargo.rs`](https://github.com/jdx/mise/blob/main/src/backend/cargo.rs).

## Dependencies

Install Rust/Cargo for source builds. A working linker and any native libraries
required by the crate must also be available. Prebuilt installs can avoid the
compilation step; see [Settings](#settings) for cargo-binstall selection and fallback.

## Usage

Declare Rust and eza together in the current project:

```sh
mise use rust@stable cargo:eza
mise exec -- eza --version
```

This records both tools in `mise.toml`:

```toml
[tools]
rust = "stable"
"cargo:eza" = "latest"
```

Add `-g` to `mise use` for global configuration. Run
`mise ls-remote cargo:eza` to choose a release, or pin a version with
`mise use cargo:eza@VERSION`, replacing `VERSION` with a listed release.

### Using Git

You can also install a package from a Git repository. This lets you
install a particular tag, branch, or commit revision. Replace the repository
and uppercase placeholders below; quote the complete tool argument:

```sh
# Install a specific tag
mise use 'cargo:https://github.com/username/demo@tag:TAG'

# Install the latest from a branch
mise use 'cargo:https://github.com/username/demo@branch:BRANCH'

# Install a specific commit revision
mise use 'cargo:https://github.com/username/demo@rev:COMMIT'
```

This runs `cargo install` with the corresponding Git options.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

Some Cargo settings are only meaningful when mise runs `cargo install`. If `cargo-binstall`
installs a prebuilt binary, Cargo build settings and `cargo install` behavior do not affect that
artifact. Set `cargo.binstall = false` when you need Cargo settings to control the install.

When mise uses external `cargo-binstall`, it disables cargo-binstall's `compile` strategy. If
cargo-binstall reports that no prebuilt artifact is available (exit code 94), mise runs
`cargo install` itself. Other cargo-binstall errors do not trigger this fallback. When
`cargo.binstall_only = true`, Cargo tools without an explicit Git source must be installed by cargo-binstall:
mise does not fall back to `cargo install`, and options that require `cargo install` produce an
error. Explicit Git sources are unaffected because they always use `cargo install --git` and are
never eligible for cargo-binstall.

By default, mise disables external `cargo-binstall`'s use of the third-party
[cargo-quickinstall](https://github.com/cargo-bins/cargo-quickinstall) artifact host. This is
separate from crate-author GitHub releases and artifacts declared in `package.metadata.binstall`.
Together with the always-disabled compile strategy, the default external cargo-binstall flag is
`--disable-strategies compile,quick-install`. Set `cargo.binstall_quickinstall = true` to allow
quick-install; mise then passes `--disable-strategies compile`. This setting does not affect mise's
native `cargo.binstall_native` path, which does not use quickinstall. Set `cargo.binstall = false`
to disable binstall entirely.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="cargo" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `cargo` backend—these
go in `[tools]` in `mise.toml`.

When `cargo-binstall` is available, mise uses it for registry installs unless a tool option needs
`cargo install` to build from source.

For options that do not skip `cargo-binstall`, mise disables cargo-binstall's compile strategy and
runs `cargo install` itself only when cargo-binstall exits with code 94 to report that no prebuilt
artifact is available.

mise records the effective `features`, `default-features`, `bin`, `crate`, and `locked` values with
each installed Cargo version. Changing any of these options reinstalls the same version instead of
reusing a binary built or selected with different options. Feature names are normalized, so changing
their order or switching between a string and an array does not trigger an unnecessary reinstall.

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

Enable crate features (passed as `cargo install --features`):

```toml
[tools]
"cargo:sqlx-cli" = { version = "latest", features = ["postgres", "rustls"] }
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

Use Cargo.lock (passes `cargo install --locked`) when building the CLI. This is the default;
pass `false` to disable it:

```toml
[tools]
"cargo:https://github.com/username/demo" = { version = "tag:v1.0.0", locked = false }
```

This option does not cause mise to skip `cargo-binstall`; it affects mise's `cargo install`
fallback when cargo-binstall reports that no prebuilt artifact is available.

## Troubleshooting

- **Compilation or linker failure:** check the first Cargo error and the crate's native build requirements. Selecting features forces a source build.
- **No executable found:** the package must publish a binary target; use `bin` or `crate` when selecting from a workspace.
- **Unexpected prebuilt binary:** inspect the binstall settings. Set `cargo.binstall = false` when you need a local build with Cargo's configuration.

The `locked` tool option uses the crate's `Cargo.lock` while building. It is
separate from [mise.lock](/dev-tools/mise-lock.html), which records the version of
the CLI installed by mise.
