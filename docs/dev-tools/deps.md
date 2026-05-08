# Deps <Badge type="warning" text="experimental" />

The `mise deps` command manages project dependencies by hashing source files
(e.g., `package-lock.json`) and running install commands when changes are detected.
It can also add and remove individual packages.

## Quick Start

```bash
# Enable experimental features
export MISE_EXPERIMENTAL=1

# Install all project dependencies
mise deps

# Add a package
mise deps add npm:react

# Add a dev dependency
mise deps add -D npm:vitest

# Remove a package
mise deps remove npm:lodash
```

## Configuration

Configure deps providers in `mise.toml`:

```toml
# Built-in npm provider (auto-detects lockfile)
[deps.npm]
auto = true  # Auto-run before mise x/run

# Built-in providers for other package managers
[deps.yarn]
[deps.pnpm]
[deps.bun]
[deps.aube]
[deps.go]
[deps.pip]
[deps.poetry]
[deps.uv]
[deps.bundler]
[deps.composer]

# Disable specific providers
[deps]
disable = ["npm"]
```

## Built-in Providers

mise includes built-in providers for common package managers:

| Provider   | Sources                                 | Outputs               | Command                              |
| ---------- | --------------------------------------- | --------------------- | ------------------------------------ |
| `npm`      | `package.json`, `package-lock.json`     | `node_modules/`       | `npm install`                        |
| `yarn`     | `package.json`, `yarn.lock`             | `node_modules/`       | `yarn install`                       |
| `pnpm`     | `package.json`, `pnpm-lock.yaml`        | `node_modules/`       | `pnpm install`                       |
| `bun`      | `package.json`, `bun.lock`, `bun.lockb` | `node_modules/`       | `bun install`                        |
| `aube`     | `package.json`, `aube-lock.yaml`        | `node_modules/`       | `aube install`                       |
| `go`       | `go.mod`                                | `vendor/` or `go.sum` | `go mod vendor` or `go mod download` |
| `pip`      | `requirements.txt`                      | `.venv/`              | `pip install -r requirements.txt`    |
| `poetry`   | `pyproject.toml`, `poetry.lock`         | `.venv/`              | `poetry install`                     |
| `uv`       | `pyproject.toml`, `uv.lock`             | `.venv/`              | `uv sync`                            |
| `bundler`  | `Gemfile`, `Gemfile.lock`               | `vendor/bundle/`      | `bundle install`                     |
| `composer` | `composer.json`, `composer.lock`        | `vendor/`             | `composer install`                   |
| `dart`     | `pubspec.yaml`, `pubspec.lock`          | `.dart_tool/`         | `dart pub get`                       |
| `flutter`  | `pubspec.yaml`, `pubspec.lock`          | `.dart_tool/`         | `flutter pub get`                    |

Built-in providers are only active when explicitly configured in `mise.toml` and their lockfile exists.

## Adding and Removing Packages

The `mise deps add` and `mise deps remove` commands let you manage individual packages
using the `ecosystem:package` syntax:

```bash
# Add packages
mise deps add npm:react
mise deps add npm:@types/react@19
mise deps add -D npm:vitest        # dev dependency

# Remove packages
mise deps remove npm:lodash
```

The ecosystem prefix tells mise which package manager to use. Currently supported
ecosystems for add/remove: `npm`, `yarn`, `pnpm`, `bun`, `aube`, `dart`, `flutter`.

## Custom Providers

Create custom providers for project-specific build steps:

```toml
[deps.codegen]
sources = ["schema/*.graphql", "codegen.yml"]
outputs = ["src/generated/"]
run = "npm run codegen"
description = "Generate GraphQL types"

[deps.prisma]
sources = ["prisma/schema.prisma"]
outputs = ["node_modules/.prisma/"]
run = "npx prisma generate"
```

### Provider Options

| Option        | Type     | Description                                                               |
| ------------- | -------- | ------------------------------------------------------------------------- |
| `auto`        | bool     | Auto-run before `mise x` and `mise run` (default: false)                  |
| `sources`     | string[] | Files/patterns to check for changes                                       |
| `outputs`     | string[] | Files/directories that must exist for the provider to be considered fresh |
| `run`         | string   | Command to run when stale                                                 |
| `env`         | table    | Environment variables to set                                              |
| `dir`         | string   | Working directory for the command                                         |
| `description` | string   | Description shown in output                                               |
| `depends`     | string[] | Other provider names that must complete before this one runs              |
| `timeout`     | string   | Timeout for the run command, e.g., `"30s"`, `"5m"` (default: no timeout)  |

## Freshness Checking

mise uses blake3 content hashing to determine if sources have changed since the last
successful run. Hashes are stored in `$MISE_STATE_DIR/deps/<hash>.toml`, keyed by
project root (so nothing is written inside the project directory).

1. Compute blake3 hashes of all source files
2. Compare against stored hashes from the last successful run
3. If any file was added, removed, or changed, the provider is stale

This means:

- If you modify `package-lock.json`, `node_modules/` will be considered stale
- If `node_modules/` doesn't exist, the provider is always stale
- If sources don't exist, the provider is considered fresh (nothing to do)
- On first run (no stored state), the provider is always considered stale

## Auto-Install

When `auto = true` is set on a provider, it will automatically run before:

- `mise run` (task execution)
- `mise x` (exec command)

This ensures dependencies are always up-to-date before running tasks or commands.

To skip auto-install for a single invocation:

```bash
mise run --no-deps build
mise x --no-deps -- npm test
```

## Staleness Warnings

When using `mise activate`, mise will warn you if any auto-enabled providers have stale dependencies:

```
mise WARN deps: npm may need update, run `mise deps`
```

This can be disabled with:

```toml
[settings]
status.show_deps_stale = false
```

## CLI Usage

```bash
# Install all project dependencies
mise deps

# Install only a specific provider
mise deps install npm

# Show why a provider is fresh or stale
mise deps install npm --explain

# Show what would run without executing
mise deps install --dry-run

# Force run even if outputs are fresh
mise deps install --force

# List available deps providers
mise deps install --list

# Skip specific providers
mise deps install --skip npm

# Add/remove packages
mise deps add npm:react
mise deps remove npm:lodash
```

## Dependencies

Providers can declare dependencies on other providers using the `depends` field. A provider
will wait for all its dependencies to complete successfully before running.

```toml
[deps.uv]
auto = true

[deps.ansible-galaxy]
auto = true
depends = ["uv"]
run = "ansible-galaxy install -r requirements.yml && touch .galaxy-installed"
sources = ["requirements.yml"]
outputs = [".galaxy-installed"]
```

In this example, `ansible-galaxy` will wait for `uv` to finish before starting.

Providers without `depends` run in parallel as before. If a dependency fails, all providers
that depend on it are skipped. Circular dependencies are detected and the affected providers
are skipped with a warning.

## Parallel Execution

Deps providers run in parallel, respecting the `jobs` setting for concurrency limits.
This speeds up installation when multiple providers need to run (e.g., both npm and pip).
Providers with `depends` will wait for their dependencies to complete before starting,
while independent providers run concurrently.

```toml
[settings]
jobs = 4  # Run up to 4 providers in parallel
```

## Example: Full-Stack Project

```toml
# mise.toml for a project with Node.js frontend and Python backend

[deps.npm]
auto = true

[deps.poetry]
auto = true

[deps.prisma]
auto = true
depends = ["npm"]  # needs node_modules first
sources = ["prisma/schema.prisma"]
outputs = ["node_modules/.prisma/"]
run = "npx prisma generate"

[deps.frontend-codegen]
depends = ["npm"]  # needs node_modules first
sources = ["schema.graphql", "codegen.ts"]
outputs = ["src/generated/"]
run = "npm run codegen"
```

Running `mise deps` will install npm and poetry dependencies in parallel, then run prisma
and frontend-codegen (also in parallel, since they only depend on npm, not each other).
