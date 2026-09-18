---
description: "mise can be used to install and manage multiple versions of bun on the same system."
---

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

## Version files

Enable [idiomatic version files](/configuration.html#idiomatic-version-files) to read
`.bun-version` or a version declaration in `package.json`:

```sh
mise settings add idiomatic_version_file_enable_tools bun
```

For example, this `package.json` selects Bun 1.2.0:

```json [package.json]
{
  "devEngines": {
    "runtime": { "name": "bun", "version": "1.2.0" }
  }
}
```

mise checks `devEngines.runtime` first, then falls back to `devEngines.packageManager` and
the top-level `packageManager` field (for example, `"packageManager": "bun@1.2.0"`).

`devEngines` runtime and package-manager declarations can be objects or arrays; mise reads
the first entry in an array. The `engines` compatibility fields are not used to select a version.
See [package-manager versions](/lang/node.html#package-manager-versions-in-package-json)
for the package-manager declaration formats.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `bun` backend.
These options go in the `[tools]` section in `mise.toml`.

### `install_env`

Set environment variables for install-time commands run by the core `bun` backend:

```toml
[tools]
bun = { version = "latest", install_env = { HTTPS_PROXY = "http://proxy.example" } }
```
