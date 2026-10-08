---
description: "Install Deno with mise and select its version from mise.toml, .deno-version, or package.json."
---

# Deno

mise installs [Deno](https://deno.com/) release binaries and selects a version
per project.

## Quick start

Install Deno for the current project and check the selected executable:

```sh
mise use deno@2
mise exec -- deno --version
```

`mise use` writes `deno = "2"` to `mise.toml`, which keeps the project on the
Deno 2 series. Use `mise use -g deno@2` for a personal default outside projects.

Update Deno with [`mise upgrade deno`](/cli/upgrade.html). `deno upgrade`
replaces the binary inside mise's install directory without changing the
version mise recorded.

## Choosing a version

`deno@2` selects the newest 2.x release, `deno@2.9.7` selects that release, and
`deno@latest` selects the newest release. List the available versions with
`mise ls-remote deno`. See [version requests](/dev-tools/versions.html) for the
full syntax.

## Version files

mise can read `.deno-version` and the `devEngines.runtime` field of
`package.json`. Enable them for Deno:

```sh
mise settings add idiomatic_version_file_enable_tools deno
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

For example, this `package.json` selects Deno 2.9.7:

```json [package.json]
{
  "devEngines": {
    "runtime": { "name": "deno", "version": "2.9.7" }
  }
}
```

mise reads `devEngines.runtime` when its `name` is `deno`. The field can be an
object or an array; mise reads the first entry of an array. The `engines`
compatibility field is not used to select a version.

## Global scripts and `DENO_INSTALL_ROOT`

mise sets `DENO_INSTALL_ROOT` to `<install>/.deno` and puts `<install>/.deno/bin`
on `PATH`, where `<install>` is the selected Deno version's install directory.
Scripts installed with `deno install -g` therefore belong to one Deno version
and are not on `PATH` after you switch versions. To share them across versions,
set the root and add its `bin` directory to `PATH`:

```toml [mise.toml]
[env]
DENO_INSTALL_ROOT = "{{env.HOME}}/.deno"
_.path = ["{{env.HOME}}/.deno/bin"]
```

A `DENO_INSTALL_ROOT` set in `[env]` takes precedence over the one mise sets.

## How mise installs Deno

mise lists Deno versions from its
[versions host](/configuration/settings.html#use_versions_host), or from
`deno.com/versions.json` when `use_versions_host` is off. It downloads the
release archive for your platform from `dl.deno.land` and runs `deno -V` to
check it. On Linux, mise installs the glibc builds that Deno publishes.
[`mise lock`](/cli/lock.html) records the checksum from the `.sha256sum` file
published next to each archive.

An installed plugin named `deno` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

Deno has no Deno-specific options. Generic options such as `install_env`,
`postinstall` and `os` work as described in
[tool options](/dev-tools/#tool-options). `install_env` reaches the `deno -V`
check and `postinstall` commands, not the download. To download through a
proxy, set `https_proxy` in the environment that runs mise (see the
[FAQ](/faq.html#how-do-i-use-mise-with-http-proxies)).
