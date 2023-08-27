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

Behind the scenes, rtx uses [`node-build`](https://github.com/nodenv/node-build) to install pre-compiled binaries and compile from source if necessary. You can check its [README](https://github.com/nodenv/node-build/blob/master/README.md) for additional settings and some troubleshooting.


## Requirements

rtx uses [node-build](https://github.com/nodenv/node-build) to install node runtimes, you need to ensure the [dependencies](https://github.com/nodenv/node-build/wiki#suggested-build-environment) are installed before installing node.


## Configuration

`node-build` already has a [handful of settings](https://github.com/nodenv/node-build#custom-build-configuration), in additional to that `rtx-node` has a few extra configuration variables:

- `RTX_NODE_BUILD_REPO` [string]: the default is `https://github.com/nodenv/node-build.git`
- `RTX_NODE_VERBOSE_INSTALL` [bool]: Enables verbose output for downloading and building.
- `RTX_NODE_FORCE_COMPILE` [bool]: Forces compilation from source instead of preferring pre-compiled binaries
- `RTX_NODE_CONCURRENCY` [uint]: How many jobs should be used in compilation. Defaults to half the computer cores
- `RTX_NODE_DEFAULT_PACKAGES_FILE` [string]: location of default packages file, defaults to `$HOME/.default-npm-packages`
- `NODEJS_ORG_MIRROR` [string]: (Legacy) overrides the default mirror used for downloading the 
  distributions, alternative to the `NODE_BUILD_MIRROR_URL` node-build env var

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
