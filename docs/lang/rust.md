# Rust

Rust/cargo can be installed which uses rustup under the hood. mise will install rustup if it is not already
installed and install the requested toolchain, components, and targets. By default, mise respects the `RUSTUP_HOME` and `CARGO_HOME` environment
variables for the home directories and falls back to their standard location (`~/.rustup` and `~/.cargo`) if they are
not set. You can change this by setting the `MISE_RUSTUP_HOME` and `MISE_CARGO_HOME` environment variables if you'd like
to isolate mise's rustup/cargo from your other rustup/cargo installations.

Unlike most tools, these won't exist inside of `~/.local/share/mise/installs` because they are managed by rustup.
mise keeps a symlink there for install tracking, sets the `RUSTUP_TOOLCHAIN` environment variable to the requested
version, and asks rustup to install any configured components or targets when you run `mise install`.

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

### `install_env`

Set environment variables for rustup install commands:

```toml
[tools]
rust = { version = "latest", install_env = { RUSTUP_DIST_SERVER = "https://static.rust-lang.org" } }
```

### `components`

The `components` option allows you to specify which components to install. Multiple components can be
specified as an array or by separating them with a comma. The set of available components may vary with different releases and
toolchains. Please consult the Rust documentation for the most up-to-date list of components.

```toml
[tools]
"rust" = { version = "1.83.0", components = ["rust-src", "llvm-tools"] }
```

If the Rust toolchain is already installed, `mise install` will still add any missing configured components.

### `profile`

The `profile` option allows you to specify the type of release to install. The following values
are supported:

- `minimal`: Includes as few components as possible to get a working compiler (`rustc`, `rust-std`, and `cargo`)
- `default`: Includes all of the components in the minimal profile, and adds `rust-docs`, `rustfmt`, and `clippy`
- `complete`: Includes all the components available through `rustup`. This should never be used, as it includes every component ever included in the metadata and thus will almost always fail.

If not set, it defaults to the profile configured in `rustup`. You can check your current default by running `rustup show profile`.

```toml
[tools]
"rust" = { version = "1.83.0", profile = "minimal" }
```

### `targets`

The `targets` option allows you to specify a list of platforms to install for cross-compilation. Multiple targets can
be specified as an array or by separating them with a comma.

```toml
[tools]
"rust" = {
  version = "1.83.0",
  targets = ["wasm32-unknown-unknown", "thumbv2-none-eabi"],
}
```

If the Rust toolchain is already installed, `mise install` will still add any missing configured targets.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="rust" :level="3" />
