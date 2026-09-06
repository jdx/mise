# Elixir

`mise` can be used to manage multiple [`elixir`](https://elixir-lang.org/) versions on the same system.

## Usage

Declare both Erlang and Elixir for the project:

```sh
mise use erlang elixir
mise exec -- elixir --version
```

[Erlang/OTP](/lang/erlang.html) is required by Elixir. The version command reports
both runtimes, which helps diagnose an incompatible pair. Use `mise ls-remote
elixir` and `mise ls-remote erlang` to choose versions supported by your project,
and pass both requests to `mise use`.

Add `-g` to set personal defaults. In an existing Mix project, run
`mise exec -- mix deps.get` to install application dependencies; selecting Elixir
does not install them.

These instructions use mise's built-in elixir support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/elixir.rs)
for backend details.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `elixir` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `elixir` backend:

```toml
[tools]
elixir = { version = "latest", install_env = { MIX_HOME = "~/.mix" } }
```
