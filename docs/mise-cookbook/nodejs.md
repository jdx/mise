# Node.js Cookbook

Use mise to select [Node.js](/lang/node.html) and your package manager, then run
the scripts and dependencies declared by the project.

## Getting started with Node.js

To install Node.js in a directory, run:

```shell
mise use node
```

This installs the latest version of Node.js and creates a `mise.toml` file with the following content:

```toml
[tools]
node = "latest"
```

To install Node.js globally instead (for example, node v26), run:

```shell
mise use -g node@26
```

## Add node modules binaries to the PATH

When you install Node.js packages listed in `package.json`, you typically need `npx` or the full path to run their binaries. For example:

```shell
mise exec -- npm install --save-dev eslint
eslint --version # doesn't work
npx eslint --version # works
```

With `mise`, you can add the node modules binaries to the `PATH`, which makes CLIs installed with npm available without `npx`.

```toml [mise.toml]
[env]
_.path = ['{{config_root}}/node_modules/.bin']
```

Example:

```shell
mise exec -- npm install --save-dev eslint
mise exec -- eslint --version # works without shell activation
```

With shell activation, `eslint --version` also works directly.

## Example Node.js Project

This recipe expects a `package.json` with `start`, `lint`, `test`, and `build`
scripts, plus a committed `package-lock.json`. Keep ESLint, TypeScript, and test
runners in the project's `devDependencies`, so npm's lockfile controls their
versions alongside the packages they use.

```toml [mise.toml]
[tools]
node = "24"

[env]
NODE_ENV = { default = "development" }

[tasks.install]
description = "Install the locked npm dependency tree"
alias = "i"
run = "npm ci"

[tasks.start]
description = "Start the development server"
alias = "s"
run = "npm run start"

[tasks.lint]
description = "Run the project's lint script"
alias = "l"
run = "npm run lint"

[tasks.test]
description = "Run the project's tests"
alias = "t"
run = "npm test"

[tasks.build]
description = "Build the project"
alias = "b"
run = "npm run build"
```

Run `mise run install` after cloning the repository, then `mise run test` or
`mise run start`. npm scripts already put `node_modules/.bin` on `PATH`, so these
tasks do not need a separate path directive. For a new project without a lockfile,
run `mise exec -- npm install` once and commit the resulting lockfile.

## Example with `pnpm`

This example uses `pnpm` as the package manager. Merge the following field into
your existing `package.json`, which must also define a `dev` script:

```json [package.json]
{
  "devEngines": {
    "packageManager": {
      "name": "pnpm",
      "version": "10.15.0"
    }
  }
}
```

The install task is skipped when `package.json`, `pnpm-lock.yaml`, and
`mise.toml` have not changed and `node_modules/.pnpm/lock.yaml` exists and is up
to date.

```toml [mise.toml]
[tools]
node = '24'

[settings]
# Read the pnpm version from package.json
idiomatic_version_file_enable_tools = ['pnpm']

[env]
_.path = ['{{config_root}}/node_modules/.bin']

[tasks.pnpm-install]
description = 'Installs dependencies with pnpm'
run = 'pnpm install'
sources = ['package.json', 'pnpm-lock.yaml', 'mise.toml']
outputs = ['node_modules/.pnpm/lock.yaml']

[tasks.dev]
description = 'Calls your dev script in `package.json`'
run = 'node --run dev'
depends = ['pnpm-install']
```

Run `mise run dev` to install the selected tools and prepare dependencies before
starting the existing application:

- `mise` will install the correct version of Node.js
- `mise` will install the `pnpm` version declared in `package.json`
- `pnpm install` runs when its sources or outputs are stale, before `node --run dev`

The timestamp check does not verify every file in `node_modules`. If dependencies
are missing or damaged, run `mise run --force pnpm-install`.

## Replacing Corepack

mise can install and select npm, pnpm, and Yarn without Corepack. The simplest
setup is to declare both Node.js and the package manager in `mise.toml`:

```toml [mise.toml]
[tools]
node = '24'
pnpm = '10.15.0'
```

To keep `package.json` as the package-manager version source, enable its
[idiomatic version file](/configuration.html#idiomatic-version-files) support:

```json [package.json]
{
  "packageManager": "pnpm@10.15.0+sha224.88208eb7c2e7de6ed534fa298248dee656723116995eda4b508fd0c9"
}
```

```toml [mise.toml]
[tools]
node = '24'

[settings]
idiomatic_version_file_enable_tools = ['pnpm']
```

Run `mise install` to install the declared versions. With shell activation,
mise's shims can also install a missing configured package manager when it is
first invoked. This uses
[`not_found_auto_install`](/configuration/settings.html#not_found_auto_install),
which is enabled by default.

Corepack-style `+sha1`, `+sha224`, `+sha256`, `+sha384`, and `+sha512` suffixes
are verified against the exact package-manager artifact before installation.
For npm, pnpm, and Yarn Classic this is the registry tarball; for modern Yarn it
is Yarn's published CLI file. Without a checksum, mise uses the package
manager's preferred registry backend (usually Aqua) and that backend's normal
verification.

Enable each package manager that a repository may declare:

```toml [mise.toml]
[settings]
idiomatic_version_file_enable_tools = ['npm', 'pnpm', 'yarn']
```

Unlike Corepack, mise does not supply a built-in "known good" package-manager
version when a project declares none. Configure the version in `mise.toml`,
`package.json`, or your global mise config instead.
