# NodeJS in rtx

The following are instructions for using the nodejs rtx core plugin. This is used when
the "experimental" setting is "true" and there isn't a git plugin installed named "nodejs"

If you want to use [asdf-nodejs](https://github.com/asdf-vm/asdf-nodejs) or [rtx-nodejs](https://github.com/rtx-plugins/rtx-nodejs) then use `rtx plugins install nodejs URL`.

The code for this is inside of the rtx repository at [`./src/plugins/core/nodejs.rs`](https://github.com/jdxcode/rtx/blob/main/src/plugins/core/nodejs.rs).

## Usage

The following installs the latest version of nodejs-18.x and makes it the global
default:

```sh-session
$ rtx install nodejs@18
$ rtx global nodejs@18
```

Behind the scenes, rtx uses [`node-build`](https://github.com/nodenv/node-build) to install pre-compiled binaries and compile from source if necessary. You can check its [README](https://github.com/nodenv/node-build/blob/master/README.md) for additional settings and some troubleshooting.


```sh-session
$ rtx global nodejs@16 nodejs@18
$ nodejs -V
16.0.0
$ nodejs.11 -V
18.0.0
```

## Configuration

`node-build` already has a [handful of settings](https://github.com/nodenv/node-build#custom-build-configuration), in additional to that `rtx-nodejs` has a few extra configuration variables:

- `RTX_NODEJS_VERBOSE_INSTALL`: Enables verbose output for downloading and building.
- `RTX_NODEJS_FORCE_COMPILE`: Forces compilation from source instead of preferring pre-compiled binaries
- `RTX_NODEJS_CONCURRENCY`: How many jobs should be used in compilation. Defaults to half the computer cores
- `RTX_NODEJS_DEFAULT_PACKAGES_FILE`: location of default packages file, defaults to `$HOME/.default-nodejs-packages`
- `NODEJS_ORG_MIRROR`: (Legacy) overrides the default mirror used for downloading the distibutions, alternative to the `NODE_BUILD_MIRROR_URL` node-build env var

## Default NodeJS packages

rtx-nodejs can automatically install a default set of npm packages right after installing a node version. To enable this feature, provide a `$HOME/.default-nodejs-packages` file that lists one package per line, for example:

```
lodash
request
express
```

You can specify a non-default location of this file by setting a `RTX_NODEJS_DEFAULT_PACKAGES_FILE` variable.

## `.nvmrc` and `.node-version` support

rtx uses a `.tool-versions` or `.rtx.toml` file for auto-switching between software versions. To ease migration, you can have also have it read an existing `.nvmrc` or `.node-version` file to find out what version of Node.js should be used. This will be used if `nodejs` isn't defined in `.tool-versions`/`.rtx.toml`.


## Running the wrapped node-build command

We provide a command for running the installed `node-build` command:

```bash
rtx nodejs nodebuild --version
```

### node-build advanced variations

`node-build` has some additional variations aside from the versions listed in `rtx ls-remote 
nodejs` (chakracore/graalvm branches and some others). As of now, we weakly support these variations. In the sense that they are available for install and can be used in a `.tool-versions` file, but we don't list them as installation candidates nor give them full attention.

Some of them will work out of the box, and some will need a bit of investigation to get them built. We are planning in providing better support for these variations in the future.

To list all the available variations run:

```bash
rtx nodejs nodebuild --definitions
```

_Note that this command only lists the current `node-build` definitions. You might want to [update the local `node-build` repository](#updating-node-build-definitions) before listing them._

### Manually updating node-build definitions

Every new node version needs to have a definition file in the `node-build` repository. 
`rtx-nodejs` already tries to update `node-build` on every new version installation, but if you 
want to update `node-build` manually for some reason you can clear the cache and list the versions:

```bash
rtx cache clean
rtx ls-remote nodejs
```
