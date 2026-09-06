# Bun

`mise` can be used to install and manage multiple versions of [bun](https://bun.sh/) on the same system.

## Usage

Install Bun for the current project and check the selected executable:

```sh
mise use bun@latest
mise exec -- bun --version
```

Use `mise use -g bun@latest` for a personal default outside projects. Commit the
project's `mise.toml` so teammates select the same version request.

See available versions with `mise ls-remote bun`.

> [!NOTE]
> Update with `mise upgrade bun`. Running `bun upgrade` changes the installed
> binary without updating mise's recorded version.

These instructions use mise's built-in bun support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/bun.rs)
for backend details.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `bun` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `bun` backend:

```toml
[tools]
bun = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
