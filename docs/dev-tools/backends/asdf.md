# asdf Backend

::: warning
asdf plugins are considered legacy. For new tools, prefer [vfox plugins](/dev-tools/backends/vfox.html) which are written in Lua, work cross-platform (including Windows), and have access to built-in modules for HTTP, JSON, HTML parsing, and more.
:::

`asdf` is the original backend for mise.

It relies on asdf plugins for each tool. asdf plugins are more risky to use because they're typically written by a single developer unrelated to the tool vendor. They also generally do not function on Windows because they're written
in bash which is often not available on Windows and the scripts generally are not written to be cross-platform.

asdf plugins are not used for tools inside the [registry](https://github.com/jdx/mise/blob/main/registry/) whenever possible. Sometimes it is not possible to use more secure backends like aqua/github because tools have complex install setups or need to export env vars.

All of these are hosted in the mise-plugins org to secure the supply chain so you do not need to rely on plugins maintained by anyone except me.

Because of the extra complexity of asdf tools and security concerns we are actively moving tools in
the registry away from asdf where possible to backends like aqua and github which don't require plugins.
That said, not all tools can function with github/aqua if they have a unique installation process or
need to set env vars other than `PATH`.

## Feature Comparison: asdf vs vfox

| Feature                         | asdf Plugins       | vfox Plugins         |
| ------------------------------- | ------------------ | -------------------- |
| **Language**                    | Bash scripts       | Lua                  |
| **Windows Support**             | ❌                 | ✅                   |
| **Built-in HTTP module**        | ❌ (requires curl) | ✅                   |
| **Built-in JSON module**        | ❌ (requires jq)   | ✅                   |
| **Built-in HTML parsing**       | ❌                 | ✅                   |
| **Built-in archive extraction** | ❌                 | ✅                   |
| **Built-in semver module**      | ❌                 | ✅                   |
| **Built-in logging**            | ❌                 | ✅                   |
| **Post-install hooks**          | ❌                 | ✅                   |
| **Security attestations**       | ❌                 | ✅ (cosign, SLSA)    |
| **Multi-tool plugins**          | ❌                 | ✅ (backend plugins) |
| **Lock file support**           | ❌                 | ✅                   |
| **Rolling version checksums**   | ❌                 | ✅                   |

## Hook Migration: asdf to vfox

| asdf Script                 | vfox Hook                | Notes                                                            |
| --------------------------- | ------------------------ | ---------------------------------------------------------------- |
| `bin/list-all`              | `Available`              | Return structured version objects instead of plain text          |
| `bin/download`              | `PreInstall`             | Return URL and checksum; mise handles the download               |
| `bin/install`               | `PostInstall`            | Runs after mise downloads and extracts the tool                  |
| `bin/exec-env`              | `EnvKeys`                | Return structured key/value pairs instead of `export` statements |
| `bin/list-legacy-filenames` | `PLUGIN.legacyFilenames` | Set in `metadata.lua` instead of a script                        |
| `bin/parse-legacy-file`     | `ParseLegacyFile`        | Return structured result instead of plain text                   |

## Writing asdf (legacy) plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).
