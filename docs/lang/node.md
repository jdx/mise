# Node

Like `nvm`, (or `volta`, `fnm` or `asdf`...), `mise` can manage multiple versions of Node.js on the same system.

> The following are instructions for using the node mise core plugin. This is used when there isn't a
> git plugin installed named "node".
> If you want to use [asdf-nodejs](https://github.com/asdf-vm/asdf-nodejs)
> then run `mise plugins install node https://github.com/asdf-vm/asdf-nodejs`

The code for this is inside the mise repository at [`./src/plugins/core/node.rs`](https://github.com/jdx/mise/blob/main/src/plugins/core/node.rs).

## Usage

The following installs the latest version of node-20.x and makes it the global
default:

```sh
mise use -g node@20
```

See the [Node.JS Cookbook](/mise-cookbook/nodejs.html) for common tasks and examples.

## `.nvmrc` and `.node-version` support

By default, mise uses a `mise.toml` file for auto-switching between software versions.

It also supports `.tool-versions`, `.nvmrc` or `.node-version` file to find out what version of Node.js should be used. This will be used if `node` isn't defined in `mise.toml`.

This makes it a drop-in replacement for `nvm`. See [idiomatic version files](/configuration.html#idiomatic-version-files) for more information.

## Default node packages

mise-node can automatically install a default set of npm packages right after installing a node version. To enable this feature, provide a `$HOME/.default-npm-packages` file that lists one package per line, for example:

```text
lodash
request
express
```

You can specify a non-default location of this file by setting a `MISE_NODE_DEFAULT_PACKAGES_FILE` variable.

## "nodejs" -> "node" Alias

You cannot install/use a plugin named "nodejs". If you attempt this, mise will just rename it to
"node". See the [FAQ](/faq.html#what-is-the-difference-between-nodejs-and-node-or-golang-and-go)
for an explanation.

## Building from source

If compiling from source, see [BUILDING.md](https://github.com/nodejs/node/blob/main/BUILDING.md#building-nodejs-on-supported-platforms) in node's documentation for
required system dependencies.

```shell
mise settings node.compile=1
mise use node@latest
```

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

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="node" :level="3" />
