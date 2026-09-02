# Bun

`mise` can be used to install and manage multiple versions of [bun](https://bun.sh/) on the same system.

> The following are instructions for using the bun mise core plugin. It is used when no
> git plugin named "bun" is installed.

The code for this is inside the mise repository at
[`./src/plugins/core/bun.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/bun.rs).

## Usage

The following installs bun and makes it the global default:

```sh
mise use -g bun@0.7     # install bun 0.7.x
mise use -g bun@latest  # install latest bun
```

See available versions with `mise ls-remote bun`.

> [!NOTE]
> Avoid upgrading bun with `bun upgrade`, since `mise` will not be aware of the change.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `bun` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `bun` backend:

```toml
[tools]
bun = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
