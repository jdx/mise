---
description: "Install Node.js with mise, read .nvmrc or package.json, and pin npm, pnpm, or Yarn per project."
---

# Node.js

mise installs Node.js from the official nodejs.org builds and selects a version
per project. It can also read `.nvmrc`, `.node-version` and `package.json`, so
projects set up for other version managers keep working.

## Quick start

Select Node.js for the current project and check it without depending on shell
activation:

```sh
mise use node@26
mise exec -- node --version
```

`mise use` writes `node = "26"` to `mise.toml`. Use `mise use -g node@26` for a
personal default; a project's version overrides it inside that project.
[`mise upgrade node`](/cli/upgrade.html) updates within the configured request.

`nodejs` is an alias for `node`; see the
[FAQ](/faq.html#what-is-the-difference-between-nodejs-and-node-or-golang-and-go).
For project recipes, see the [Node.js cookbook](/mise-cookbook/nodejs.html).

## Choosing a version

| Request            | Selects                                                           |
| ------------------ | ----------------------------------------------------------------- |
| `node@26`          | The newest 26.x release                                           |
| `node@26.10.0`     | That release                                                      |
| `node@lts`         | The newest release of the current LTS line                        |
| `node@lts/krypton` | The newest release of a named LTS line (`lts-krypton` also works) |
| `node@latest`      | The newest release                                                |

List the available versions with `mise ls-remote node`, or one release line with
`mise ls-remote node 26`. See [version requests](/dev-tools/versions.html) for
the full syntax.

## Version files {#nvmrc-node-version-and-package-json-support}

mise does not read `.nvmrc`, `.node-version` or `package.json` until you enable
them for Node.js:

```sh
mise settings add idiomatic_version_file_enable_tools node
```

This changes your global config. Add `--local` to enable it in the project's
`mise.toml` instead, so teammates get the same behavior. See
[idiomatic version files](/dev-tools/versions.html#idiomatic-version-files).

`.nvmrc` aliases such as `lts/*` and `lts/iron` work as they do in nvm, and a
leading `v` is ignored. To read `.nvmrc` and `.node-version` but not
`package.json`:

```sh
mise settings add idiomatic_version_file_disable_files node:package.json
```

### `package.json`

mise reads `devEngines.runtime` when its `name` is `node`:

```json [package.json]
{
  "devEngines": {
    "runtime": { "name": "node", "version": "26.10.0" }
  }
}
```

This selects Node.js 26.10.0. `devEngines.runtime` can be an object or an
array; mise reads the first entry of an array. See the
[Bun](/lang/bun.html#version-files) and [Deno](/lang/deno.html#version-files)
pages for other runtimes.

mise does not read `engines.node`, which describes the Node.js versions a
package is compatible with, not the version to develop with. If a project only
has an `engines` range, select a version explicitly:

```sh
mise use node@24
```

## Package managers

npm ships with Node.js. mise can also install and select npm, pnpm and Yarn
as separate tools, without Corepack, and [aube](https://aube.sh/), which
works with existing npm, pnpm and Yarn lockfiles. See the
[Node.js cookbook](/mise-cookbook/nodejs.html) for project recipes with pnpm
and aube.

### Pin the npm version {#pinning-npm-version}

To keep a team on the same npm, and avoid `package-lock.json` churn between npm
versions, pin npm next to Node.js:

```toml [mise.toml]
[tools]
node = "26"
npm = "11"
```

To write exact versions for both:

```sh
mise use --pin node@lts npm@latest
```

This writes the resolved versions to `mise.toml`. Inside the mise environment,
the separately configured npm takes precedence over the npm bundled with
Node.js; check it with `mise exec -- npm --version`. This selects the npm
executable; the project's lockfile still controls its dependencies.

### Package-manager versions in `package.json`

mise can read the npm, pnpm or Yarn version from `package.json`. Enable each
package manager that a repository may declare:

```toml [mise.toml]
[settings]
idiomatic_version_file_enable_tools = ["npm", "pnpm", "yarn"]
```

mise reads a matching `devEngines.packageManager` declaration first, then the
top-level `packageManager` field. The declaration's name must match the enabled
tool:

```json [package.json]
{
  "packageManager": "pnpm@10.15.0"
}
```

This selects pnpm 10.15.0. The equivalent `devEngines` declaration is:

```json [package.json]
{
  "devEngines": {
    "packageManager": { "name": "pnpm", "version": "10.15.0" }
  }
}
```

`devEngines.packageManager` can also be an array; mise reads its first entry.
For Bun's runtime and package-manager precedence, see
[Bun version files](/lang/bun.html#version-files).

A Corepack-style checksum suffix (`+sha1`, `+sha224`, `+sha256`, `+sha384` or
`+sha512`) is verified against the exact package-manager artifact before
installation:

```json [package.json]
{
  "packageManager": "pnpm@10.15.0+sha224.88208eb7c2e7de6ed534fa298248dee656723116995eda4b508fd0c9"
}
```

For npm, pnpm and Yarn 1, that artifact is the npm registry tarball. For Yarn 2
and later it is Yarn's published CLI file, and the declaration must name an
exact Yarn version. Without a checksum, mise installs the package manager
through its usual [registry](/registry.html) backend and that backend's
verification.

Unlike Corepack, mise does not pick a known-good package-manager version when a
project declares none. Set the version in `mise.toml`, `package.json` or your
global config.

### Corepack

mise installs package managers itself, so Corepack is not needed. If you still
want Corepack's shims, set [`node.corepack`](/lang/node.html#node.corepack) to `true`; mise
then runs `corepack enable` after installing any Node.js version that includes
Corepack.

## Global npm packages

`npm install -g` installs into the active Node.js version, unless your
`.npmrc` sets a different `prefix`. A globally installed command is therefore
not available after you switch Node.js versions.

On macOS and Linux, mise replaces each version's `bin/npm` with a small wrapper
that runs [`mise reshim`](/cli/reshim.html) after global installs and
uninstalls, so new commands get [shims](/dev-tools/shims.html). Turn it off with
[`node.npm_shim`](/lang/node.html#node.npm_shim).

To keep a CLI across Node.js versions, install it as its own tool with the
[npm backend](/dev-tools/backends/npm.html), for example
`mise use -g npm:typescript`.

## How mise installs Node.js

mise downloads the official binary archive for your platform from
[`node.mirror_url`](/lang/node.html#node.mirror_url) (`https://nodejs.org/dist/` by default),
checks it against the release's `SHASUMS256.txt`, and, for Node.js 20 and
later, verifies the OpenPGP signature on that file
([`node.gpg_verify`](/lang/node.html#node.gpg_verify)). It then runs `node -v` and `npm -v`.
mise sets no Node.js-specific environment variables; the version's `bin`
directory goes on `PATH`.

When no binary exists for your platform, mise compiles Node.js from source,
unless [`node.compile`](/lang/node.html#node.compile) is `false`.

On Alpine and NixOS, mise currently compiles from source by default, because
[`all_compile`](/configuration/settings.html#all_compile) defaults to `true`
there. This default is deprecated, and mise 2027.8.0 switches to precompiled
binaries. To use precompiled binaries now, set `node.compile = false` or
`all_compile = false`; on NixOS, enable [nix-ld](https://github.com/Mic92/nix-ld)
first. To keep compiling, set `all_compile = true` explicitly.

An installed plugin named `node` takes precedence over the built-in
installer. If mise behaves differently from this page, check
[`mise plugins ls`](/cli/plugins/ls.html) and see
[selecting another implementation](/core-tools.html#selecting-another-implementation).

### Compiling from source {#building-from-source}

Set `node.compile` to build Node.js instead of downloading a binary. Install
Node.js's
[build dependencies](https://github.com/nodejs/node/blob/main/BUILDING.md#building-nodejs-on-supported-platforms)
first:

```sh
mise settings node.compile=true
mise install node@26
```

mise uses `ninja` when it is on `PATH`. The [`node.ninja`](/lang/node.html#node.ninja),
[`node.concurrency`](/lang/node.html#node.concurrency),
[`node.configure_opts`](/lang/node.html#node.configure_opts), [`node.cflags`](/lang/node.html#node.cflags)
and [`node.apply_patches`](/lang/node.html#node.apply_patches) settings tune the build.

### Unofficial builds

The [unofficial builds](https://unofficial-builds.nodejs.org/) project
publishes Node.js for platforms the official builds do not cover.

On musl hosts such as Alpine, mise detects musl and, when it downloads a binary
rather than compiling (see the Alpine note above), fetches Node.js's `musl`
flavor from the unofficial builds automatically while `node.mirror_url` is the
default. The [`libc`](/configuration/settings.html#libc) setting set to `musl`
has the same effect.

For other platforms, such as linux-loong64 or linux-armv6l, or for the
`glibc-217` flavor built against an older glibc, point mise at the unofficial
builds:

```sh
mise settings node.mirror_url=https://unofficial-builds.nodejs.org/download/release/
```

Then choose a flavor with [`node.flavor`](/lang/node.html#node.flavor) if you need one:

```sh
mise settings node.flavor=glibc-217
```

## Migrating from nvm, nodenv, or Homebrew

To reuse Node.js versions you already installed, link them into mise with
[`mise sync node`](/cli/sync/node.html):

```sh
mise sync node --nvm     # reads versions under node.nvm_dir (NVM_DIR)
mise sync node --nodenv  # reads versions under node.nodenv_root (NODENV_ROOT)
mise sync node --brew    # reads Homebrew's versioned node@NN formulae
```

`mise sync` does not overwrite versions mise installed itself. Then enable
`.nvmrc` and `.node-version` as described in
[Version files](#nvmrc-node-version-and-package-json-support).

## Tool options

Node.js has no Node.js-specific options. `install_env` reaches source builds,
the `node -v` and `npm -v` checks, default package installs, `corepack enable`
and `postinstall` commands. For example, to override compiler flags for a
source build:

```toml [mise.toml]
[tools]
node = { version = "26", install_env = { CFLAGS = "-O2" } }
```

Other generic options are described in [tool options](/dev-tools/#tool-options).

## Default packages file <Badge type="danger" text="deprecated" /> {#default-node-packages}

mise installs the packages listed in `~/.default-npm-packages`
([`node.default_packages_file`](/lang/node.html#node.default_packages_file)), one per line,
with `npm install --global` into each new Node.js version. mise warns about this
file from 2026.11.0 and stops reading it in 2027.11.0. Install npm CLIs with
the [npm backend](/dev-tools/backends/npm.html) instead, for example
`"npm:typescript" = "latest"`, or use a
[`postinstall`](/dev-tools/#tool-postinstall-commands) command for packages
every Node.js version needs.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="node" :level="3" />
