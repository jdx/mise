# Node in rtx

The following are instructions for using the node rtx core plugin. This is used when there isn't a
git plugin installed named "node".

If you want to use [asdf-nodejs](https://github.com/asdf-vm/asdf-nodejs) or
[rtx-node](https://github.com/rtx-plugins/rtx-nodejs) then run `rtx plugins install node GIT_URL`.

The code for this is inside the rtx repository at [`./src/plugins/core/node.rs`](https://github.com/jdx/rtx/blob/main/src/plugins/core/node.rs).

## Usage

The following installs the latest version of node-20.x and makes it the global
default:

```sh-session
$ rtx use -g node@20
```

## Requirements

See [BUILDING.md](https://github.com/nodejs/node/blob/main/BUILDING.md#building-nodejs-on-supported-platforms) in node's documentation for 
required system dependencies.

## Configuration

- `RTX_NODE_BUILD` [bool]: See [Moving away from node-build](#moving-away-from-node-build) below.
- `RTX_NODE_BUILD_REPO` [string]: the default is `https://github.com/nodenv/node-build.git`
- `RTX_NODE_VERIFY` [bool]: Verify the downloaded assets using GPG. Defaults to `true`.
- `RTX_NODE_NINJA` [bool]: Use ninja instead of make to compile nodel. Defaults to `true` if installed.
- `RTX_NODE_FORCE_COMPILE` [bool]: Forces compilation from source instead of preferring pre-compiled binaries. Can also be set across all languages with [`RTX_NODE_FORCE_COMPILE`](https://github.com/jdx/rtx#rtx_node_force_compile1)
- `RTX_NODE_CONCURRENCY` [uint]: How many jobs should be used in compilation. Defaults to half the computer cores
- `RTX_NODE_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-npm-packages`
- `RTX_NODE_MIRROR_URL` [string]: overrides the default mirror used for downloading the distributions
- `RTX_NODE_CFLAGS` [string]: Additional CFLAGS options (e.g., to override -O3).
- `RTX_NODE_CONFIGURE_OPTS` [string]: Additional `./configure` options.
- `RTX_NODE_MAKE_OPTS` [string]: Additional `make` options.
- `RTX_NODE_MAKE_INSTALL_OPTS` [string]: Additional `make install` options.

## Default node packages

rtx-node can automatically install a default set of npm packages right after installing a node version. To enable this feature, provide a `$HOME/.default-npm-packages` file that lists one package per line, for example:

```
lodash
request
express
```

You can specify a non-default location of this file by setting a `RTX_NODE_DEFAULT_PACKAGES_FILE` variable.

## `.nvmrc` and `.node-version` support

rtx uses a `.tool-versions` or `.rtx.toml` file for auto-switching between software versions. To ease migration, you can have also have it read an existing `.nvmrc` or `.node-version` file to find out what version of Node.js should be used. This will be used if `node` isn't defined in `.tool-versions`/`.rtx.toml`.

## Running the wrapped node-build command

We provide a command for running the installed `node-build` command:

```bash
rtx node node-build --version
```

### node-build advanced variations

`node-build` has some additional variations aside from the versions listed in `rtx ls-remote 
node` (chakracore/graalvm branches and some others). As of now, we weakly support these variations. In the sense that they are available for install and can be used in a `.tool-versions` file, but we don't list them as installation candidates nor give them full attention.

Some of them will work out of the box, and some will need a bit of investigation to get them built. We are planning in providing better support for these variations in the future.

To list all the available variations run:

```bash
rtx node node-build --definitions
```

_Note that this command only lists the current `node-build` definitions. You might want to [update the local `node-build` repository](#updating-node-build-definitions) before listing them._

### Manually updating node-build definitions

Every new node version needs to have a definition file in the `node-build` repository.
`rtx-node` already tries to update `node-build` on every new version installation, but if you
want to update `node-build` manually for some reason you can clear the cache and list the versions:

```bash
rtx cache clean
rtx ls-remote node
```

## "nodejs" -> "node" Alias

You cannot install/use a plugin named "nodejs". If you attempt this, rtx will just renamed it to
"node". See the [FAQ](https://github.com/jdx/rtx#what-is-the-difference-between-nodejs-and-node-or-golang-and-go)
for an explanation.

## Moving away from node-build

This project is in the process of migrating away from using [node-build](https://github.com/nodenv/node-build) for fetching/compiling node.
The main reason for this is just to reduce the # of moving parts but it has some other advantages like not relying on new node-build releases to
get the latest node releases.

Currently, you can opt into using the pure-rtx node fetching with `RTX_NODE_BUILD=0` this will be the default
behavior in a few weeks from this writing.
