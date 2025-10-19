# Elixir

`mise` can be used to manage multiple [`elixir`](https://elixir-lang.org/) versions on the same system.

> The following are instructions for using the elixir core plugin. This is used when there isn't a git plugin installed named "elixir".

The code for this is inside the mise repository at
[`./src/plugins/core/elixir.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/elixir.rs).

## Usage

Use the latest stable version of elixir:

```sh
mise use -g erlang elixir
```

Note that [`erlang`](/lang/erlang.html) is required to install `elixir`.
