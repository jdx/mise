# Rust <Badge type="warning" text="experimental" />

Rust/cargo can be installed which uses rustup under the hood. mise will install rustup if it is not already
installed and add the requested targets. By default, mise respects the `RUSTUP_HOME` and `CARGO_HOME` environment
variables for the home directories and falls back to their standard location (`~/.rustup` and `~/.cargo`) if they are
not set. You can change this by setting the `MISE_RUSTUP_HOME` and `MISE_CARGO_HOME` environment variables if you'd like
to isolate mise's rustup/cargo from your other rustup/cargo installations.

Unlike most tools, these won't exist inside of `~/.local/share/mise/installs` because they are managed by rustup.
All mise does is set the `RUSTUP_TOOLCHAIN` environment variable to the requested version and rustup will
automatically install it if it doesn't exist.

## Usage

Use the latest stable version of rust:

```sh
mise use -g rust
cargo build
```

Use the latest beta version of rust:

```sh
mise use -g rust@beta
cargo build
```

Use a specific version of rust:

```sh
mise use -g rust@1.82
cargo build
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `rust` backend—these
go in `[tools]` in `mise.toml`.

### `components`

The `components` option allows you to specify which components to install. Multiple components can be
specified by separating them with a comma. The set of available components may vary with different releases and
toolchains. Please consult the Rust documentation for the most up-to-date list of components.

```toml
[tools]
"rust" = { version = "1.83.0", components = "rust-src,llvm-tools" }
```

### `profile`

The `profile` option allows you to specify the type of release to install. The following values
are supported:

- `minimal`: Includes as few components as possible to get a working compiler (rustc, rust-std, and cargo)
- `default` (default): Includes all of components in the minimal profile, and adds rust-docs, rustfmt, and clippy
- `complete`: Includes all the components available through rustup. This should never be used, as it includes every component ever included in the metadata and thus will almost always fail.

```toml
[tools]
"rust" = { version = "1.83.0", profile = "minimal" }
```

### `targets`

The `targets` option allows you to specify a list of platforms to install for cross-compilation. Multiple targets can
be specified by separating them with a comma.

```toml
[tools]
"rust" = { version = "1.83.0", targets = "wasm32-unknown-unknown,thumbv2-none-eabi" }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="rust" :level="3" />
