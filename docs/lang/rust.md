---
description: "mise can install Rust/cargo using rustup under the hood."
---

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

Install the latest stable toolchain for the current project and verify it:

```sh
mise use rust
mise exec -- rustc --version
mise exec -- cargo --version
```

In a Cargo project, use `mise exec -- cargo build` or a mise task. Add `-g` to
`mise use` for a personal default. These examples select the toolchain through
mise; they do not require shell activation.

Use the latest beta version of Rust:

```sh
mise use rust@beta
mise exec -- cargo build
```

Use the rolling nightly channel:

```sh
mise use rust@nightly
mise exec -- cargo build
```

The configuration remains `nightly`, while mise resolves the current Rust channel manifest to a concrete
`nightly-YYYY-MM-DD` toolchain for installation and lockfiles. This keeps the configured channel rolling while making
locked installs reproducible. Run `mise upgrade rust` or `mise lock --bump` to advance the locked nightly.

To keep a specific nightly instead, configure its date explicitly:

```sh
mise use rust@nightly-2026-08-13
```

An explicitly dated nightly is an exact pin. Commands using `--bump`, such as `mise upgrade --bump rust`, can replace
that pin with the current nightly.

Use a specific version of Rust:

```sh
mise use rust@1.82
mise exec -- cargo build
```

## Existing rustup projects

If the project already uses `rust-toolchain.toml`, enable idiomatic-file discovery
instead of duplicating a conflicting Rust version in `mise.toml`:

```sh
mise settings add idiomatic_version_file_enable_tools rust
mise install
mise exec -- rustup show active-toolchain
```

mise sets `RUSTUP_TOOLCHAIN` for its selected toolchain. Use `mise exec` when
comparing selection with a standalone rustup invocation, since the environment
can change which override rustup sees.

## Share Cargo builds with Mr Boxington

[Mr Boxington](https://mr-boxington.jdx.dev/) (`mbx`) is a Rust build cache and scheduler.
It reuses matching compilations across projects, worktrees, and CI, so a fresh checkout can benefit from
work you've already built. Parallel Cargo commands share a CPU and memory budget, and the cache prunes itself.
You keep using ordinary Cargo commands; no cache server is needed for local use.
See the [benchmarks](https://mr-boxington.jdx.dev/benchmarks) for examples.

Enable the `mr_boxington` tool option and install mbx as a separate tool:

```sh
mise use --tool-option mr_boxington=true rust mr-boxington
```

This writes the equivalent of:

```toml [mise.toml]
[tools]
rust = { version = "latest", mr_boxington = true }
mr-boxington = "latest"
```

Cargo commands run through mbx in `mise exec`, tasks, activated shells, and
mise shims. No `mbx setup` or postinstall hook is needed. mbx uses its normal
mise version selection and lockfile entry, independently of Rust.

```sh
mise exec -- cargo build
```

Editors and coding agents must invoke mise's Cargo shim or use `mise exec`.
Direct calls to rustup's Cargo proxy or a toolchain's Cargo binary bypass mise.
Safe mode ignores the opt-in from project-scoped Rust entries.

An explicit `[wrappers.cargo]` configuration takes precedence over this option.
The [generic command wrapper configuration](/dev-tools/shims.html#command-wrappers)
remains available when Rust is managed outside mise.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `rust` backend—these
go in `[tools]` in `mise.toml`.

### `mr_boxington`

Set `mr_boxington = true` to wrap Cargo with Mr Boxington. Defaults to `false`.
Requires `mr-boxington` in the active tool configuration; merely having `mbx` on
PATH is not sufficient. Keep its version in a separate `[tools]` entry.

The option applies to the first platform-supported Rust version in the selected
configuration. Set it to `false` in a project's Rust entry to disable an inherited
opt-in. An explicitly configured Cargo wrapper is unaffected.

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
the `minimal` or `default` profile.

### `targets`

The `targets` option specifies platforms to install for cross-compilation. Multiple targets can
be given as an array or as a comma-separated string.

```toml
[tools]
rust = {
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
