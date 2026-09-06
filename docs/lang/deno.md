# Deno

`mise` can be used to install and manage multiple versions of [deno](https://deno.land/) on the same system.

## Usage

Install Deno for the current project and verify the selected executable:

```sh
mise use deno@latest
mise exec -- deno --version
```

Use `mise use -g deno@latest` for a personal default outside projects. A specific
version request such as `deno@2` keeps the project within that release series.

See available versions with `mise ls-remote deno`.

> [!NOTE]
> Update with `mise upgrade deno`. Running `deno upgrade` changes the installed
> binary without updating mise's recorded version.

These instructions use mise's built-in deno support. An installed external
plugin with the same name can change the behavior; use `mise plugins ls` to
check for overrides. See the [core implementation](https://github.com/jdx/mise/blob/main/src/plugins/core/deno.rs)
for backend details.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `deno` backend.
These options go in the `[tools]` section of `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `deno` backend:

```toml
[tools]
deno = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
