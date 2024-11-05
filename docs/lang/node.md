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

mise uses a `.tool-versions` or `.mise.toml` file for auto-switching between software versions.
To ease migration, you can have also have it read an existing `.nvmrc` or `.node-version` file to find out what version of Node.js should be used.
This will be used if `node` isn't defined in `.tool-versions`/`.mise.toml`.

## "nodejs" -> "node" Alias

You cannot install/use a plugin named "nodejs". If you attempt this, mise will just rename it to
"node". See the [FAQ](https://github.com/jdx/mise#what-is-the-difference-between-nodejs-and-node-or-golang-and-go)
for an explanation.

## Unofficial Builds

Nodejs.org offers a set of [unofficial builds](https://unofficial-builds.nodejs.org/) which are
compatible with some platforms are not supported by the official binaries. These are a nice alternative to
compiling from source for these platforms.

To use, first set the mirror url to point to the unofficial builds:

```sh
mise settings set node.mirror_url https://unofficial-builds.nodejs.org/download/release/
```

If your goal is to simply support an alternative arch/os like linux-loong64 or linux-armv6l, this is
all that is required. Node also provides flavors such as musl or glibc-217 (an older glibc version
than what the official binaries are built with).

To use these, set `node.flavor`:

```sh
mise settings set node.flavor musl
mise settings set node.flavor glibc-217
```

## Migrating from `nvm` to `mise`

As indicated above, mise can read `.nvmrc` files to determine the required Node.js version.
This will help migrating from `nvm`.

### Example setup

For the migration example, we will consider the following NodeJS setup:

- Node.JS 20 and 22 are installed globally using `nvm`
- Node 22 is used by default
- There are two projects, one using Node 17 and the other using Node 18

The project directories contain `.nvmrc` files:

```text
- node-project-17/.nvmrc: 17
- node-project-18/.nvmrc: 18
```

:::details Here is what you one might have done using `nvm`

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
source ~/.bashrc

nvm install 22
nvm install 20
nvm alias default 22

mkdir -p node-18-project && cd node-18-project && echo "18" > .nvmrc && nvm install && cd -
mkdir -p node-17-project && cd node-17-project && echo "17" > .nvmrc && nvm install && cd -
```

:::

### Stop loading `nvm`

If it's not already done, install `mise` (see [Getting Started](/getting-started)).

Open your shell configuration file (`.bashrc`, `.zshrc`, ...) and remove or comment out the `nvm` initialization script:

```shell
# Comment out or remove these lines
# export NVM_DIR="$HOME/.nvm"...
```

then restart your shell.

:::details If you are using `bash`, this is how you can do it

```bash
curl https://mise.run | sh
echo 'eval "$(~/.local/bin/mise activate bash)"' >> ~/.bashrc
source ~/.bashrc

sed -i.bak '/NVM_DIR\|nvm.sh\|bash_completion/d' ~/.bashrc
# start a new shell
```

:::

### Migration Options

You now have two options for migration:

#### Option A: Clean Installation (Remove nvm)

1. Unload `nvm` and remove its directory:

   ```shell
   nvm_dir="${NVM_DIR:-~/.nvm}"
   nvm unload
   rm -rf "$nvm_dir"
   ```

2. Reinstall your global Node.js version with `mise`:

   ```shell
   mise use -g node@22
   ```

   This will install Node.js 22 globally and set it as the default version.
   If you also want to install Node.js 20, you can run `mise install node@20` without setting it as the default version.

3. Install Node.js 17 and 18 for the projects:

   ```shell
   cd node-project-17
   mise install

   cd ../node-project-18
   mise install
   ```

#### Option B: Keep Existing `nvm` Installations (Symlink)

This option is useful if you want to keep your existing `nvm` installations and use them with `mise`. (You won't need to reinstall global packages for example.)

1. Sync existing `nvm` installations with `mise`:

   ```bash
   mise sync node --nvm
   ```

2. Verify the sync by listing installations with `mise ls`:

   :::details `mise ls`

   ```shell
   Tool  Version            Config Source Requested
   node  17.9.1 (symlink)
   node  18.20.4 (symlink)
   node  20.11.0 (symlink)
   node  22.1.0 (symlink)
   ```

   :::

3. If you navigate to `node-project-17` and `node-project-18`, you will see that the correct Node.js version is used.
   :::details `mise ls`

   ```shell
   node-18-project# mise ls
   Tool  Version            Config Source           Requested
   node  17.9.1 (symlink)
   node  18.20.4 (symlink)  /node-18-project/.nvmrc 18
   node  22.11.0 (symlink)
   node  23.1.0 (symlink)
   node-18-project# node -v
   v18.20.4
   ```

   :::

4. Let's now create a new project with Node 19:

   ```shell
   mkdir node-project-19 && cd node-project-19
   mise use node@19 # create a mise.toml file with node@19
   cat mise.toml
   mise ls
   ```

   :::details `mise ls`

   ```shell
   Tool  Version            Config Source               Requested
   node  17.9.1 (symlink)
   node  18.20.4 (symlink)
   node  19.9.0             /node-project-19/.mise.toml 19
   node  22.11.0 (symlink)
   node  23.1.0 (symlink)
   ```
