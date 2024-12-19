# Mise + Node.js Cookbook

Here are some tips on managing Node.js projects with mise.

## Example Node.js Project

```toml [mise.toml]
min_version = "2024.9.5"

[env]
# Add the node modules bin path to the PATH
# This will make CLIs installed with npm available (without `npx`)
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
post_install = 'corepack enable'

[env]
_.path = ['./node_modules/.bin']

[tasks.pnpmInstall]
description = 'Installs dependencies with pnpm'
run = 'pnpm install'
sources = ['package.json', 'pnpm-lock.yaml', 'mise.toml']
outputs = ['node_modules/.pnpm/lock.yaml']

[tasks.dev]
description = 'Calls your dev script in `package.json`'
run = 'node --run dev'
depends = ['pnpmInstall']
```

With this setup, getting started in a NodeJS project is as simple as running `mise dev`:

- `mise` will install the correct version of NodeJS
- `mise` will enable `corepack`
- `pnpm install` will be run before `node --run dev`
