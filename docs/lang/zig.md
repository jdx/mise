---
description: "Install Zig with mise from ziglang.org or its mirrors, including nightly and Mach versions."
---

# Zig

mise installs [Zig](https://ziglang.org/) from ziglang.org or its community
mirrors and verifies each download against the Zig Software Foundation's
signing key.

## Quick start

Install Zig for the current project and check the selected compiler:

```sh
mise use zig@0.17
mise exec -- zig version
```

Use `mise use -g zig@0.17` for a personal default.

## Choosing a version

| Request           | Selects                                         |
| ----------------- | ----------------------------------------------- |
| `zig@0.17`        | The newest release in the 0.17 series           |
| `zig@latest`      | The newest stable release                       |
| `zig@master`      | The current nightly build                       |
| `zig@mach-latest` | The newest version nominated by the Mach engine |

`mise ls-remote zig` lists the ziglang.org releases.

### Nightly builds (`master`)

`zig@master` follows Zig's nightly builds. mise installs the nightly that is
current at install time under its dev version (an `X.Y.0-dev.N+commit` build).
[`mise outdated`](/cli/outdated.html) reports newer nightlies, and
[`mise upgrade zig`](/cli/upgrade.html) installs the current one.

### Mach versions

[Mach](https://machengine.org/) nominates its own Zig versions, which
`mise ls-remote zig` does not list. List them from Mach's version index (this
needs `curl` and `jq`), then install one by name, such as `zig@mach-latest`:

```sh
curl --fail --show-error --silent --location https://machengine.org/zig/index.json | jq 'keys'
```

## Version files

mise can read `.zig-version`. Enable it for Zig:

```sh
mise settings add idiomatic_version_file_enable_tools zig
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

## Zig language server (ZLS) {#zig-language-server}

The Zig language server, [ZLS](https://github.com/zigtools/zls), is a separate
tool. Install it with a version that matches your Zig:

```sh
mise use zig@0.16 zls@0.16
mise exec -- zls --version
```

ZLS releases can trail Zig's, so the newest Zig may have no matching ZLS yet.
Check `mise ls-remote zls` and use a Zig version that has one.

Installing both at `latest` does not check that they are compatible; see the
[ZLS installation guide](https://zigtools.org/zls/install/). There is no
Mach-specific ZLS release.

## How mise installs Zig

mise reads the release index at `ziglang.org/download/index.json`, or Mach's
index for Mach versions. With [`zig.use_community_mirrors`](/lang/zig.html#zig.use_community_mirrors)
on, it tries the community mirrors listed by ziglang.org, in random order,
before ziglang.org itself; the mirror list is cached for a day. Whichever
server it downloads from, mise checks the archive's minisign
signature against the Zig Software Foundation's key, then runs `zig version`.
mise sets no Zig environment variables.

An installed plugin named `zig` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

## Tool options

Zig has no Zig-specific options. Generic options such as `install_env`,
`postinstall` and `os` work as described in
[tool options](/dev-tools/#tool-options). `install_env` reaches the
`zig version` check and `postinstall` commands, not the download. To download
through a proxy, set `https_proxy` in the environment that runs mise (see the
[FAQ](/faq.html#how-do-i-use-mise-with-http-proxies)).

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="zig" :level="3" />
