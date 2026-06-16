# Erlang

`mise` can be used to install and manage multiple versions of [erlang](https://www.erlang.org/) on the same system.

> The following are instructions for using the erlang core plugin.
> This is used when there isn't a git plugin installed named "erlang".

The code for this is inside the mise repository at
[`./src/plugins/core/erlang.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/erlang.rs).

## Usage

The following installs erlang and makes it the global default:

```sh
mise use -g erlang@26
```

See available versions with `mise ls-remote erlang`.

## kerl

The plugin uses [kerl](https://github.com/kerl/kerl) under the hood to build erlang.
See kerl's docs for information on configuring kerl.

On GitHub Actions Linux runners, `ImageOS=ubuntu24`, `ImageOS=ubuntu22`, and `ImageOS=ubuntu20`
map to the precompiled Erlang build targets `ubuntu-24.04`, `ubuntu-22.04`, and
`ubuntu-20.04`. Unsupported values disable precompiled lockfile URL resolution in the
default `erlang.compile` mode, so locked installs can fall back to kerl.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `erlang` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for kerl build/install commands and other install-time commands run by
the core `erlang` backend:

```toml
[tools]
erlang = { version = "latest", install_env = { KERL_CONFIGURE_OPTIONS = "--without-javac" } }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="erlang" :level="3" />
