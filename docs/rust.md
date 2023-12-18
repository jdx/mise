# Rust in rtx

The following are instructions for using the rust rtx core plugin. This is used when there isn't a
git plugin installed named "rust".

If you want to use [asdf-rust](https://github.com/asdf-community/asdf-rust)
then use `rtx plugins install rust GIT_URL`.

The code for this is inside the rtx repository at
[`./src/plugins/core/rust.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/rust.rs).

## Usage

The following installs the latest version of rust-1.74.x (if some version of 1.74.x is not already
installed) and makes it the global default:

```sh-session
rtx use -g rust@1.74
```

## Configuration

- `RTX_RUST_WITHOUT` [string]: comma-separated list of toolchain components to omit
