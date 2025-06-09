# Mise + Node.js Cookbook

Here are some tips on managing [Node.js](/lang/node.html) projects with mise.

## Getting started with Node.js

To install Node.JS, in a directory, you can use the following command:

```shell
mise use node
```

This will install the latest version of Node.js and create a `mise.toml` file with the following content:

```toml
node = "latest"
```

If you want to install Node.JS globally instead (for example, node v22), you can use the following command:

```shell
mise use -g node@22
```

## Add node modules binaries to the PATH

When installing Node.js packages specified in `package.json`, you typically need to use `npx` or the full path to the binary. For example:

```shell
npm install --save eslint
eslint --version # doesn't work
npx eslint --version # works
```

Thanks to `mise`, you can add the node modules binaries to the `PATH`. This will make CLIs installed with npm available without `npx`.

```toml [mise.toml]
[env]
_.path = ['{{config_root}}/node_modules/.bin']
```

Example:

```shell
npm install --save eslint
eslint --version # works
```

## Example Node.js Project

```toml [mise.toml]
min_version = "2024.9.5"

[env]
_.path = ['{{config_root}}/node_modules/.bin']

# Use the project name derived from the current directory
PROJECT_NAME = "{{ config_root | basename }}"

# Set up the path for node module binaries
BIN_PATH = "{{ config_root }}/node_modules/.bin"

NODE_ENV = "{{ env.NODE_ENV | default(value='development') }}"

[tools]
# Install Node.js using the specified version
node = "{{ env['NODE_VERSION'] | default(value='lts') }}"

# Install some npm packages globally if needed
"npm:typescript" = "latest"
"npm:eslint" = "latest"
"npm:jest" = "latest"

[tasks.install]
alias = "i"
description = "Install npm dependencies"
run = "npm install"

[tasks.start]
alias = "s"
description = "Start the development server"
run = "npm run start"

[tasks.lint]
alias = "l"
description = "Run ESLint"
run = "eslint src/"

[tasks.test]
description = "Run tests"
alias = "t"
run = "jest"

[tasks.build]
description = "Build the project"
alias = "b"
run = "npm run build"

[tasks.info]
description = "Print project information"
run = '''
echo "Project: $PROJECT_NAME"
echo "NODE_ENV: $NODE_ENV"
'''
```

## Example with `pnpm`

This example uses `pnpm` as the package manager. This will skip installing dependencies if the lock file hasn't changed.

```toml [mise.toml]
[tools]
node = '22'

[hooks]
# Enabling corepack will install the `pnpm` package manager specified in your package.json
# alternatively, you can also install `pnpm` with mise
postinstall = 'npx corepack enable'

[settings]
# This must be enabled to make the hooks work
experimental = true

[env]
_.path = ['./node_modules/.bin']

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

With this setup, getting started in a NodeJS project is as simple as running `mise dev`:

- `mise` will install the correct version of NodeJS
- `mise` will enable `corepack`
- `pnpm install` will be run before `node --run dev`
