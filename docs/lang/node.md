# Node

The following are instructions for using the node mise core plugin. This is used when there isn't a
git plugin installed named "node".

If you want to use [asdf-nodejs](https://github.com/asdf-vm/asdf-nodejs)
then run `mise plugins install node https://github.com/asdf-vm/asdf-nodejs`

The code for this is inside the mise repository at [`./src/plugins/core/node.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/node.rs).

## Usage

The following installs the latest version of node-20.x and makes it the global
default:

```sh
mise use -g node@20
```

## Requirements

See [BUILDING.md](https://github.com/nodejs/node/blob/main/BUILDING.md#building-nodejs-on-supported-platforms) in node's documentation for
required system dependencies.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="node" :level="3" />

### Environment Variables

- `MISE_NODE_VERIFY` [bool]: Verify the downloaded assets using GPG. Defaults to `true`.
- `MISE_NODE_NINJA` [bool]: Use ninja instead of make to compile node. Defaults to `true` if installed.
- `MISE_NODE_CONCURRENCY` [uint]: How many jobs should be used in compilation. Defaults to half the computer cores
- `MISE_NODE_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-npm-packages`
- `MISE_NODE_CFLAGS` [string]: Additional CFLAGS options (e.g., to override -O3).
- `MISE_NODE_CONFIGURE_OPTS` [string]: Additional `./configure` options.
- `MISE_NODE_MAKE_OPTS` [string]: Additional `make` options.
- `MISE_NODE_MAKE_INSTALL_OPTS` [string]: Additional `make install` options.
- `MISE_NODE_COREPACK` [bool]: Installs the default corepack shims after installing any node version that ships with [corepack](https://github.com/nodejs/corepack).

::: info
TODO: these env vars should be migrated to compatible settings in the future.
:::

## Default node packages

mise-node can automatically install a default set of npm packages right after installing a node version. To enable this feature, provide a `$HOME/.default-npm-packages` file that lists one package per line, for example:

```text
lodash
request
express
```

You can specify a non-default location of this file by setting a `MISE_NODE_DEFAULT_PACKAGES_FILE` variable.

## `.nvmrc` and `.node-version` support

mise uses a `.tool-versions` or `mise.toml` file for auto-switching between software versions. To ease migration, you can have also have it read an existing `.nvmrc` or `.node-version` file to find out what version of Node.js should be used. This will be used if `node` isn't defined in `.tool-versions`/`.mise.toml`.

## "nodejs" -> "node" Alias

You cannot install/use a plugin named "nodejs". If you attempt this, mise will just rename it to
"node". See the [FAQ](https://github.com/jdx/mise#what-is-the-difference-between-nodejs-and-node-or-golang-and-go)
for an explanation.

## Unofficial Builds

Nodejs.org offers a set of [unofficial builds](https://unofficial-builds.nodejs.org/) which are
compatible with some platforms that are not supported by the official binaries. These are a nice alternative to
compiling from source for these platforms.

To use, first set the mirror url to point to the unofficial builds:

```sh
mise settings node.mirror_url=https://unofficial-builds.nodejs.org/download/release/
```

If your goal is to simply support an alternative arch/os like linux-loong64 or linux-armv6l, this is
all that is required. Node also provides flavors such as musl or glibc-217 (an older glibc version
than what the official binaries are built with).

To use these, set `node.flavor`:

```sh
mise settings node.flavor=musl
mise settings node.flavor=glibc-217
```
