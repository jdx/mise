---
description: "Install Rust toolchains with mise through rustup, with components, targets and rust-toolchain.toml."
---

# Rust

mise installs Rust toolchains through rustup, installing rustup first if
needed, and sets `RUSTUP_TOOLCHAIN` so `cargo` and `rustc` use the toolchain
your config selects.

## Quick start

Install the newest stable Rust for the current project and check it:

```sh
mise use rust
mise exec -- rustc --version
mise exec -- cargo --version
```

In a Cargo project, run `mise exec -- cargo build` or a mise task. These
commands select the toolchain through mise and do not need shell activation.
Add `-g` to `mise use` for a personal default.

## Choosing a version

| Request                   | Selects                                                        |
| ------------------------- | -------------------------------------------------------------- |
| `rust` or `rust@latest`   | The newest stable release, such as `1.99.0`                    |
| `rust@1.99`               | The newest 1.99.x release                                      |
| `rust@stable`             | rustup's `stable` channel, updated in place by `mise upgrade`  |
| `rust@beta`               | rustup's `beta` channel, updated in place by `mise upgrade`    |
| `rust@nightly`            | The current nightly, installed and locked as a dated toolchain |
| `rust@nightly-2026-08-13` | That nightly                                                   |

A `nightly` request stays `nightly` in your config, but mise installs the
current dated toolchain, such as `nightly-2026-10-07`, and records that in
`mise.lock`, so locked installs are reproducible.
[`mise upgrade rust`](/cli/upgrade.html) or `mise lock --bump` moves a locked
nightly forward. A dated nightly is an exact pin; `--bump`, as in
`mise upgrade --bump rust`, replaces it with the current nightly.

mise also keeps rustup's own `nightly` toolchain in step with the dated one, so
`cargo +nightly` works. It never replaces a `nightly` to which you added
components or targets with rustup, and after that `rustup update nightly` moves
it without touching the dated toolchain mise installed.

## Version files

If the project has a `rust-toolchain.toml`, let mise read it instead of
repeating the version in `mise.toml`:

```sh
mise settings add idiomatic_version_file_enable_tools rust
mise install
mise exec -- rustup show active-toolchain
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. mise reads the
`channel`, `profile`, `components` and `targets` keys of the file's
`[toolchain]` table. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

Because mise sets `RUSTUP_TOOLCHAIN`, which rustup ranks above
`rust-toolchain.toml`, the toolchain mise selects wins inside the mise
environment. A `rust` entry in your global config therefore overrides a
project's `rust-toolchain.toml` unless mise reads that file. Check what Cargo
uses with `mise exec -- rustup show active-toolchain`.

## Cargo, rustup and installed binaries {#where-rustup-and-cargo-live}

rustup and Cargo keep their state in `RUSTUP_HOME` and `CARGO_HOME`, by default
`~/.rustup` and `~/.cargo`. mise uses those variables from your environment
when they are set. To keep mise's toolchains apart from another rustup
installation, set [`rust.rustup_home`](/lang/rust.html#rust.rustup_home) and
[`rust.cargo_home`](/lang/rust.html#rust.cargo_home), which take precedence over them:

```toml [mise.toml]
[settings.rust]
rustup_home = "~/.local/share/rustup"
cargo_home = "~/.local/share/cargo"
```

The settings can also come from the `MISE_RUSTUP_HOME` and `MISE_CARGO_HOME`
environment variables. A `RUSTUP_HOME` or `CARGO_HOME` set in `[env]` takes
precedence over all of these.

When `~/.rustup` and `~/.cargo` have not been set up and no home is configured,
mise reuses a rustup installed by a package manager such as Homebrew, APT or
pacman. Your `PATH` must contain a directory with the `rustup`, `cargo` and
`rustc` proxies. With an explicit Rust or Cargo home, mise runs its own rustup
setup instead.

`cargo install` puts binaries in `$CARGO_HOME/bin`, which is on `PATH` and
shared by every toolchain. To pin a Rust CLI per project, install it with the
[`cargo:` backend](/dev-tools/backends/cargo.html), such as
`mise use cargo:ripgrep`.

## Environment variables

| Variable           | Value                                    |
| ------------------ | ---------------------------------------- |
| `RUSTUP_TOOLCHAIN` | The selected toolchain, such as `1.99.0` |
| `RUSTUP_HOME`      | The rustup home described above          |
| `CARGO_HOME`       | The Cargo home described above           |

mise also puts `$CARGO_HOME/bin`, or the package manager's proxy directory, on
`PATH`.

## Cache Cargo builds with Mr Boxington {#share-cargo-builds-with-mr-boxington}

[Mr Boxington](https://mr-boxington.jdx.dev/) (`mbx`) caches Rust compilations
across projects, worktrees and CI, and schedules parallel Cargo commands within
a shared CPU and memory budget. See its
[benchmarks](https://mr-boxington.jdx.dev/benchmarks). Turn on the
`mr_boxington` tool option and add mbx as a separate tool:

```sh
mise use --tool-option mr_boxington=true rust mr-boxington
```

This writes:

```toml [mise.toml]
[tools]
rust = { version = "latest", mr_boxington = true }
mr-boxington = "latest"
```

Cargo then runs through mbx under `mise exec`, in tasks, in activated shells
and through mise's shims, with no `mbx setup` or postinstall step. Calls that
bypass mise, such as rustup's Cargo proxy or a toolchain's `cargo` binary, do
not use it, so point editors and coding agents at mise's Cargo shim or
`mise exec`. Having `mbx` on `PATH` is not enough; `mr-boxington` must be in the
active `[tools]`.

The option applies to the first Rust version the configuration selects for this
platform. mbx keeps its own version and lockfile entry, independent of Rust. In
[safe mode](/security.html#safe-mode), project config cannot turn the option
on. To turn off an opt-in inherited from another config, set
`mr_boxington = false` in the project's Rust entry.

An explicit [`[wrappers.cargo]`](/dev-tools/shims.html#command-wrappers) takes
precedence over the option. When Rust is managed outside mise, configure that
wrapper directly.

## How mise installs Rust

mise downloads `rustup-init` from `sh.rustup.rs` (`win.rustup.rs` on Windows)
when rustup is not set up, then runs `rustup toolchain install` with the
configured profile, components and targets. The toolchains live in
`RUSTUP_HOME`, not in mise's installs directory; mise keeps a link there to
track the version. When the toolchain is already installed, `mise install`
still adds missing configured components and targets.
[`rust.default_host`](/lang/rust.html#rust.default_host) sets the host triple passed to
`rustup-init`.

An installed plugin named `rust` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

### `components`

Components to install, as an array or a comma-separated string. Run
`mise exec -- rustup component list` to see what a toolchain offers.

```toml [mise.toml]
[tools]
rust = { version = "1.99", components = ["rust-src", "llvm-tools"] }
```

### `profile`

The rustup profile to install:

- `minimal`: `rustc`, `rust-std` and `cargo`
- `default`: the minimal profile plus `rust-docs`, `rustfmt` and `clippy`
- `complete`: every component available through rustup; this profile includes
  every component ever published in the metadata and almost always fails

Without it, mise uses rustup's configured profile; check it with
`rustup show profile`.

```toml [mise.toml]
[tools]
rust = { version = "1.99", profile = "minimal" }
```

### `targets`

Platforms to install for cross-compilation, as an array or a comma-separated
string:

```toml [mise.toml]
[tools]
rust = {
  version = "1.99",
  targets = ["wasm32-unknown-unknown", "thumbv7em-none-eabi"],
}
```

### `mr_boxington`

Runs Cargo through Mr Boxington. See
[Cache Cargo builds with Mr Boxington](#share-cargo-builds-with-mr-boxington).

### `install_env`

Sets environment variables for `rustup-init` and `rustup toolchain install`.
rustup downloads the toolchain itself, so variables it reads, such as a mirror
in `RUSTUP_DIST_SERVER`, apply:

```toml [mise.toml]
[tools]
rust = { version = "latest", install_env = { RUSTUP_DIST_SERVER = "https://rust-mirror.example.com" } }
```

Other generic options are described in [tool options](/dev-tools/#tool-options).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="rust" :level="3" />
