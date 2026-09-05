# Rust

mise can install Rust/cargo using rustup under the hood. It installs rustup if it is not already installed, then
installs the requested toolchain, components, and targets. By default, mise respects the `RUSTUP_HOME` and `CARGO_HOME` environment
variables for the home directories and falls back to their standard locations (`~/.rustup` and `~/.cargo`) if they are
not set. To isolate mise's rustup/cargo from your other rustup/cargo installations, set the `MISE_RUSTUP_HOME` and
`MISE_CARGO_HOME` environment variables instead.

These variables can also be set in mise configuration. They are applied to Rust operations in the same mise invocation:

```toml
[env]
MISE_RUSTUP_HOME = "{{env.HOME}}/.local/share/rustup"
MISE_CARGO_HOME = "{{env.HOME}}/.local/share/cargo"
```

Explicit `RUSTUP_HOME` and `CARGO_HOME` values in `[env]` take precedence over their corresponding `MISE_` variables.

When the standard Rust homes have not been initialized and no home override is configured, mise can also reuse a
package-manager installation of rustup. The original `PATH` must contain a directory with the `rustup`, `cargo`, and
`rustc` proxies, as provided by package managers such as Homebrew, APT, and pacman. An explicit Rust or Cargo home
continues to use mise's managed rustup initialization instead of an external proxy directory.

Unlike most tools, Rust toolchains are not stored in `~/.local/share/mise/installs` because rustup manages them.
mise keeps a symlink there for install tracking, sets the `RUSTUP_TOOLCHAIN` environment variable to the requested
version, and asks rustup to install any configured components or targets when you run `mise install`.

## Usage

Use the latest stable version of Rust:

```sh
mise use -g rust
cargo build
```

Use the latest beta version of Rust:

```sh
mise use -g rust@beta
cargo build
```

Use the rolling nightly channel:

```sh
mise use -g rust@nightly
cargo build
```

The configuration remains `nightly`, while mise resolves the current Rust channel manifest to a concrete
`nightly-YYYY-MM-DD` toolchain for installation and lockfiles. This keeps the configured channel rolling while making
locked installs reproducible. Run `mise upgrade rust` or `mise lock --bump` to advance the locked nightly.

To keep a specific nightly instead, configure its date explicitly:

```sh
mise use -g rust@nightly-2026-08-13
```

An explicitly dated nightly is an exact pin. Commands using `--bump`, such as `mise upgrade --bump rust`, can replace
that pin with the current nightly.

Use a specific version of Rust:

```sh
mise use -g rust@1.82
cargo build
```

## Share Cargo builds with Mr Boxington

[Mr Boxington](https://mr-boxington.jdx.dev/) (`mbx`) gives every checkout on a machine one shared,
self-pruning compilation cache. A crate compiled in one worktree can be reused in another, and concurrent Cargo
commands share a CPU and memory budget instead of oversubscribing the machine. It can also share cached artifacts
with teammates and CI runners through a cache server, S3, or GitHub Actions.

Install `mbx` and configure mise's [`cargo` command wrapper](/dev-tools/shims.html#command-wrappers) to use it:

```toml [mise.toml]
[tools]
rust = "latest"
mr-boxington = "latest"

[wrappers.cargo]
command = "mbx"
env = { MBX_CARGO_SHIM_MODE = "1" }
```

Run `mise reshim` after adding the wrapper. Existing commands and mise tasks can keep invoking `cargo` normally:

```toml [mise.toml]
[tasks.build]
run = "cargo build"
```

Within the mise environment, the wrapper transparently routes those commands through `mbx`. This also avoids
rewriting every task as `mbx build`, and keeps the same tasks usable if the wrapper is later removed.

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

The `components` option specifies which components to install. Multiple components can be
given as an array or as a comma-separated string. The set of available components varies between releases and
toolchains; consult the Rust documentation for the current list.

```toml
[tools]
"rust" = { version = "1.83.0", components = ["rust-src", "llvm-tools"] }
```

If the Rust toolchain is already installed, `mise install` will still add any missing configured components.

### `profile`

The `profile` option specifies the rustup profile to install. The following values
are supported:

- `minimal`: Includes as few components as possible to get a working compiler (`rustc`, `rust-std`, and `cargo`)
- `default`: Includes all of the components in the minimal profile, and adds `rust-docs`, `rustfmt`, and `clippy`
- `complete`: Includes all the components available through `rustup`. Avoid this profile: it includes every component ever included in the metadata and will almost always fail.

If not set, it defaults to the profile configured in `rustup`. You can check your current default by running `rustup show profile`.

```toml
[tools]
"rust" = { version = "1.83.0", profile = "minimal" }
```

If the Rust toolchain is already installed, `mise install` restores missing components implied by
the `minimal`, `default`, or `complete` profile. Complete-profile membership comes from the
installed toolchain's rustup manifest because it can vary between Rust releases.

Rustup supports only those three named profiles and discourages using `complete`. To customize a
profile, use the `components` and `targets` options with `minimal` or `default`.

### `targets`

The `targets` option specifies platforms to install for cross-compilation. Multiple targets can
be given as an array or as a comma-separated string.

```toml
[tools]
"rust" = {
  version = "1.83.0",
  targets = ["wasm32-unknown-unknown", "thumbv7em-none-eabi"],
}
```

If the Rust toolchain is already installed, `mise install` will still add any missing configured targets.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="rust" :level="3" />
