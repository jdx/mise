---
description: "mise can be used to install and manage multiple versions of deno on the same system."
---

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

## Version files

Enable [idiomatic version files](/configuration.html#idiomatic-version-files) to read
`.deno-version` or a version declaration in `package.json`:

```sh
mise settings add idiomatic_version_file_enable_tools deno
```

For example, this `package.json` selects Deno 2.2.0:

```json [package.json]
{
  "devEngines": {
    "runtime": { "name": "deno", "version": "2.2.0" }
  }
}
```

mise reads `devEngines.runtime` when its `name` is `deno`.

`devEngines.runtime` accepts an object or an array; mise reads the first entry in an array.
The `engines` compatibility fields are not used to select a version.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `deno` backend.
These options go in the `[tools]` section of `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `deno` backend:

```toml
[tools]
deno = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
