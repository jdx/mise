---
description: "Pin Node.js and a package manager per project, run package scripts as tasks, and replace Corepack."
---

# Node.js

Pin Node.js and your package manager for a project, and run its package scripts
as mise tasks. For installing and selecting Node.js itself, see
[Node.js](/lang/node.html).

## Wrap npm scripts in tasks {#example-node-js-project}

This recipe expects a `package.json` with `start`, `lint`, `test` and `build`
scripts, plus a committed `package-lock.json`. Keep ESLint, TypeScript and test
runners in the project's `devDependencies`, so the lockfile controls their
versions along with the packages they use.

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

The tasks always run with the pinned Node.js, behave the same in CI, and show up
next to other languages' tasks in `mise tasks`.
[`NODE_ENV = { default = "development" }`](/environments/#defaults) sets the
variable only when it is not already set, so CI can still export
`NODE_ENV=production`.

Run `mise run install` after cloning the repository, then `mise run test` or
`mise run start`. npm scripts put `node_modules/.bin` on `PATH` themselves, so
these tasks need no path directive. For a new project without a lockfile, run
`mise exec -- npm install` once and commit `package-lock.json`.

## Run package binaries without npx {#add-node-modules-binaries-to-the-path}

Binaries from `devDependencies` live in `node_modules/.bin`, which is not on
`PATH`, so you normally call them through `npx`:

```sh
mise exec -- npm install --save-dev eslint
mise exec -- npx eslint --version
```

Add that directory to `PATH` with [`_.path`](/environments/#env-path):

```toml [mise.toml]
[env]
_.path = ["{{config_root}}/node_modules/.bin"]
```

Now `mise exec -- eslint --version` works, and so does `eslint --version` in a
shell with mise activated.

## Install pnpm dependencies before a task {#example-with-pnpm}

This example uses pnpm, with its version declared in `package.json`. Merge this
field into a `package.json` that also defines a `dev` script:

```json [package.json]
{
  "devEngines": {
    "packageManager": {
      "name": "pnpm",
      "version": "12.9.1"
    }
  }
}
```

```toml [mise.toml]
[tools]
node = "24"

[settings]
# Read the pnpm version from package.json
idiomatic_version_file_enable_tools = ["pnpm"]

[tasks.pnpm-install]
description = "Install dependencies with pnpm"
run = "pnpm install"
sources = ["package.json", "pnpm-lock.yaml", "mise.toml"]
outputs = ["node_modules/.pnpm/lock.yaml"]

[tasks.dev]
description = "Run the dev script from package.json"
run = "node --run dev"
depends = ["pnpm-install"]
```

`mise run dev` installs the pinned Node.js and the pnpm version from
`package.json`, runs `pnpm install` if `package.json`, `pnpm-lock.yaml` or
`mise.toml` changed since `node_modules/.pnpm/lock.yaml` was last written, then
runs `node --run dev`. `node --run` puts `node_modules/.bin` on `PATH` for the
script, so the project needs no `_.path` entry.

This [freshness check](/tasks/caching.html) compares modification times and does
not inspect the rest of `node_modules`. If dependencies are missing or damaged,
run `mise run --force pnpm-install`.

Two alternatives handle the freshness check for you: the experimental
[`[deps.pnpm]`](/dev-tools/deps.html) provider, or aube, described next.

## Run projects with aube {#run-projects-with-aube}

[aube](https://aube.sh/) is a Node.js package manager that reads and writes
existing npm, pnpm, Yarn and Bun lockfiles in place, so you can try it in a
project without migrating the lockfile. Its `aubr` command runs a package
script and first installs dependencies when they are missing or stale; when
they are current, it skips the install.

Install it with mise, then run an existing package script:

```sh
mise use aube
mise exec -- aubr test
```

With aube, the pnpm example above needs no install task: a task with
`run = "aubr dev"` replaces both tasks. See
[aube's security overview](https://aube.sh/security) for its release-age,
trust-policy, malicious-package and lifecycle-script protections.

## Replace Corepack {#replacing-corepack}

Node.js 25 and later no longer include Corepack, and earlier versions ship it
disabled. mise installs and selects npm, pnpm and Yarn as tools, so a project
does not need Corepack to get the package manager it declares. The simplest
setup declares both Node.js and the package manager in `mise.toml`:

```toml [mise.toml]
[tools]
node = "24"
pnpm = "12"
```

To keep `package.json` as the source of the version, enable it as an
[idiomatic version file](/dev-tools/versions.html#idiomatic-version-files) for
that package manager with
[`idiomatic_version_file_enable_tools`](/configuration/settings.html#idiomatic_version_file_enable_tools).
[Package-manager versions](/lang/node.html#package-manager-versions-in-package-json)
lists the fields mise reads, how it verifies a checksum, and what happens when a
repository declares no version.

Run `mise install` to install the declared versions. In a shell with
`mise activate`, mise's command-not-found handler (and an existing shim, if an
earlier version is installed) also installs a missing configured package
manager the first time you run it. This uses the
[`not_found_auto_install`](/configuration/settings.html#not_found_auto_install)
setting.

To keep using Corepack's own shims, see [Corepack](/lang/node.html#corepack).
