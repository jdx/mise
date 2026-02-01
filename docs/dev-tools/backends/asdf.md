# asdf Backend

> [!WARNING]
> asdf plugins are considered legacy. For new plugin development, use [vfox (tool plugins)](./vfox.md) instead. Vfox plugins are written in Lua, work on all platforms including Windows, and have access to [built-in modules](/plugin-lua-modules.html) for HTTP, JSON, HTML parsing, and more.

`asdf` is the original backend for mise.

It relies on asdf plugins for each tool. asdf plugins are more risky to use because they're typically written by a single developer unrelated to the tool vendor. They also generally do not function on Windows because they're written
in bash which is often not available on Windows and the scripts generally are not written to be cross-platform.

asdf plugins are not used for tools inside the [registry](https://github.com/jdx/mise/blob/main/registry/) whenever possible. Sometimes it is not possible to use more secure backends like aqua/ubi because tools have complex install setups or need to export env vars.

All of these are hosted in the mise-plugins org to secure the supply chain so you do not need to rely on plugins maintained by anyone except me.

Because of the extra complexity of asdf tools and security concerns we are actively moving tools in
the registry away from asdf where possible to backends like aqua and ubi which don't require plugins.
That said, not all tools can function with ubi/aqua if they have a unique installation process or
need to set env vars other than `PATH`.

## Feature Comparison: asdf vs vfox

| Feature                   | asdf                        | vfox                      |
| ------------------------- | --------------------------- | ------------------------- |
| Windows support           | ❌                          | ✅                        |
| Built-in HTTP requests    | ❌ (shell out to curl)      | ✅ (`http` module)        |
| Built-in JSON parsing     | ❌ (shell out to jq)        | ✅ (`json` module)        |
| HTML parsing              | ❌                          | ✅ (`html` module)        |
| Archive extraction        | ❌ (shell out to tar/unzip) | ✅ (`archiver` module)    |
| Semantic version sorting  | ❌                          | ✅ (`semver` module)      |
| Structured logging        | ❌ (echo to stderr)         | ✅ (`log` module)         |
| Post-install hooks        | ❌                          | ✅                        |
| Security attestations     | ❌                          | ✅ (GitHub, cosign, SLSA) |
| Multiple tools per plugin | ❌                          | ✅ (backend plugins)      |
| Cross-platform lock files | ❌                          | ✅                        |
| Rolling version checksums | ❌                          | ✅                        |
| Language                  | Bash                        | Luau                      |

## Hook Migration: asdf to vfox

If you're migrating an asdf plugin to vfox, use this mapping:

| asdf Script                    | vfox Hook                         | Notes                                 |
| ------------------------------ | --------------------------------- | ------------------------------------- |
| `bin/list-all`                 | `Available`                       | Returns structured version tables     |
| `bin/download` + `bin/install` | `PreInstall` + `PostInstall`      | Separates URL resolution from install |
| `bin/exec-env`                 | `EnvKeys`                         | Returns structured key-value pairs    |
| `bin/parse-legacy-file`        | `ParseLegacyFile`                 | Same concept                          |
| `bin/list-legacy-filenames`    | `legacyFilenames` in metadata.lua | Declarative                           |
| `bin/list-bin-paths`           | `EnvKeys` (PATH entries)          | Handled via PATH in EnvKeys           |

## Writing asdf (legacy) plugins for mise

See the asdf documentation for more information on [writing plugins](https://asdf-vm.com/plugins/create.html).
