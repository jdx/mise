# Elixir

`mise` can be used to manage multiple [`elixir`](https://elixir-lang.org/) versions on the same system.

> The following are instructions for using the elixir mise core plugin. It is used when no
> git plugin named "elixir" is installed.

The code for this is inside the mise repository at
[`./src/plugins/core/elixir.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/elixir.rs).

## Usage

Use the latest stable version of elixir:

```sh
mise use -g erlang elixir
```

[`erlang`](/lang/erlang.html) is required to install `elixir`.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `elixir` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `elixir` backend:

```toml
[tools]
elixir = { version = "latest", install_env = { MIX_HOME = "~/.mix" } }
```
