---
description: "Install Bun with mise and select its version from mise.toml, .bun-version, or package.json."
---

# Bun

mise installs [Bun](https://bun.sh/) release binaries, including `bunx`, and
selects a version per project.

## Quick start

Install Bun for the current project and check the selected executable:

```sh
mise use bun@1
mise exec -- bun --version
```

`mise use` writes `bun = "1"` to `mise.toml`. Commit that file so teammates get
the same version request, and use `mise use -g bun@1` for a personal default
outside projects.

Update Bun with [`mise upgrade bun`](/cli/upgrade.html). `bun upgrade` replaces
the binary inside mise's install directory without changing the version mise
recorded.

## Choosing a version

`bun@1` selects the newest 1.x release, `bun@1.3.14` selects that release, and
`bun@latest` selects the newest release. List the available versions with
`mise ls-remote bun`. See [version requests](/dev-tools/versions.html) for the
full syntax.

## Version files

mise can read `.bun-version` and the version declarations in `package.json`.
Enable them for Bun:

```sh
mise settings add idiomatic_version_file_enable_tools bun
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

For example, this `package.json` selects Bun 1.3.14:

```json [package.json]
{
  "devEngines": {
    "runtime": { "name": "bun", "version": "1.3.14" }
  }
}
```

mise reads `devEngines.runtime` first, then `devEngines.packageManager`, then
the top-level `packageManager` field (for example,
`"packageManager": "bun@1.3.14"`). Each `devEngines` entry can be an object or
an array; mise reads the first entry of an array. The `engines` compatibility
field is not used to select a version.

When a `packageManager` declaration for the same version carries a
Corepack-style checksum, such as `bun@1.3.14+sha224.…`, mise installs Bun from
the npm registry and verifies the package against that checksum. See
[package-manager versions](/lang/node.html#package-manager-versions-in-package-json)
for the declaration formats.

## Global packages

mise sets no Bun environment variables. `bun add -g` installs packages under
Bun's own global directory (`~/.bun`, or `$BUN_INSTALL`) and their executables
in `~/.bun/bin`. mise does not add that `bin` directory to `PATH`. To install a CLI that every project can use, add it
as a tool with the [npm backend](/dev-tools/backends/npm.html), for example
`mise use -g npm:prettier`.

## How mise installs Bun

mise downloads the release archive for your platform from
[Bun's GitHub releases](https://github.com/oven-sh/bun/releases) and runs
`bun -v` to check it. On x64 CPUs without AVX2, mise picks Bun's `baseline`
build. On musl systems such as Alpine, or with the [`libc`](/configuration/settings.html#libc)
setting set to `musl`, it picks the `musl` build. Windows on arm64 gets the
native build from Bun 1.3.10 on, and the x64 baseline build for older versions.

[`mise lock`](/cli/lock.html) records the checksum Bun publishes in
`SHASUMS256.txt` for each platform.

An installed plugin named `bun` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

Bun has no Bun-specific options. Generic options such as `install_env`,
`postinstall` and `os` work as described in
[tool options](/dev-tools/#tool-options). `install_env` reaches the `bun -v`
check and `postinstall` commands, not the download. To download through a
proxy, set `https_proxy` in the environment that runs mise (see the
[FAQ](/faq.html#how-do-i-use-mise-with-http-proxies)).
