# Cargo Backend <Badge type="warning" text="experimental" />

You may install packages directly from [Cargo Crates](https://crates.io/) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/forge/cargo.rs`](https://github.com/jdx/mise/blob/main/src/forge/cargo.rs).

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

## Configuration

Set these with `mise settings set [VARIABLE] [VALUE]` or by setting the environment variable listed.

### `cargo_binstall`

* Type: `bool`
* Env: `MISE_CARGO_BINSTALL`
* Default: `true`

If true, mise will use `cargo binstall` instead of `cargo install` if
[`cargo-binstall`](https://crates.io/crates/cargo-binstall) is installed and on PATH.
This makes installing CLIs with cargo _much_ faster by downloading precompiled binaries.

You can install it with mise:

```sh
mise use -g cargo-binstall
```
