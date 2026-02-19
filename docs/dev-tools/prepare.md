# Prepare <Badge type="warning" text="experimental" />

The `mise prepare` command ensures project dependencies are ready by checking if lockfiles
are newer than installed outputs (e.g., `package-lock.json` vs `node_modules/`) and running
install commands if needed.

## Quick Start

```bash
# Enable experimental features
export MISE_EXPERIMENTAL=1

# Run all applicable prepare steps
mise prepare

# Or use the alias
mise prep
```

## Configuration

Configure prepare providers in `mise.toml`:

```toml
# Built-in npm provider (auto-detects lockfile)
[prepare.npm]
auto = true  # Auto-run before mise x/run

# Built-in providers for other package managers
[prepare.yarn]
[prepare.pnpm]
[prepare.bun]
[prepare.go]
[prepare.pip]
[prepare.poetry]
[prepare.uv]
[prepare.bundler]
[prepare.composer]

# Custom provider
[prepare.codegen]
auto = true
sources = ["schema/*.graphql"]
outputs = ["src/generated/"]
run = "npm run codegen"

# Disable specific providers
[prepare]
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
| `go`       | `go.mod`                                | `vendor/` or `go.sum` | `go mod vendor` or `go mod download` |
| `pip`      | `requirements.txt`                      | `.venv/`              | `pip install -r requirements.txt`    |
| `poetry`   | `pyproject.toml`, `poetry.lock`         | `.venv/`              | `poetry install`                     |
| `uv`       | `pyproject.toml`, `uv.lock`             | `.venv/`              | `uv sync`                            |
| `bundler`  | `Gemfile`, `Gemfile.lock`               | `vendor/bundle/`      | `bundle install`                     |
| `composer` | `composer.json`, `composer.lock`        | `vendor/`             | `composer install`                   |

Built-in providers are only active when explicitly configured in `mise.toml` and their lockfile exists.

## Custom Providers

Create custom providers for project-specific build steps:

```toml
[prepare.codegen]
sources = ["schema/*.graphql", "codegen.yml"]
outputs = ["src/generated/"]
run = "npm run codegen"
description = "Generate GraphQL types"

[prepare.prisma]
sources = ["prisma/schema.prisma"]
outputs = ["node_modules/.prisma/"]
run = "npx prisma generate"
```

### Provider Options

| Option          | Type     | Description                                                                     |
| --------------- | -------- | ------------------------------------------------------------------------------- |
| `auto`          | bool     | Auto-run before `mise x` and `mise run` (default: false)                        |
| `sources`       | string[] | Files/patterns to check for changes                                             |
| `outputs`       | string[] | Files/directories that should be newer than sources                             |
| `run`           | string   | Command to run when stale                                                       |
| `env`           | table    | Environment variables to set                                                    |
| `dir`           | string   | Working directory for the command                                               |
| `description`   | string   | Description shown in output                                                     |
| `touch_outputs` | bool     | Touch output mtimes after a successful run so they appear fresh (default: true) |

## Freshness Checking

mise uses modification time (mtime) comparison to determine if outputs are stale:

1. Find the most recent mtime among all source files
2. Find the most recent mtime among all output files
3. If any source is newer than all outputs, the provider is stale

This means:

- If you modify `package-lock.json`, `node_modules/` will be considered stale
- If `node_modules/` doesn't exist, the provider is always stale
- If sources don't exist, the provider is considered fresh (nothing to do)

After a successful run, mise touches the mtime of each output to now (controlled by
`touch_outputs`, default `true`). This ensures that commands which are no-ops when
dependencies are already satisfied (e.g. `uv sync` when the venv is up to date) still
mark outputs as fresh, preventing repeated stale warnings on subsequent invocations.

## Auto-Prepare

When `auto = true` is set on a provider, it will automatically run before:

- `mise run` (task execution)
- `mise x` (exec command)

This ensures dependencies are always up-to-date before running tasks or commands.

To skip auto-prepare for a single invocation:

```bash
mise run --no-prepare build
mise x --no-prepare -- npm test
```

## Staleness Warnings

When using `mise activate`, mise will warn you if any auto-enabled providers have stale dependencies:

```
mise WARN prepare: npm may need update, run `mise prep`
```

This can be disabled with:

```toml
[settings]
status.show_prepare_stale = false
```

## CLI Usage

```bash
# Run all applicable prepare steps
mise prepare

# Show what would run without executing
mise prepare --dry-run

# Force run even if outputs are fresh
mise prepare --force

# List available prepare providers
mise prepare --list

# Run only specific providers
mise prepare --only npm --only codegen

# Skip specific providers
mise prepare --skip npm
```

## Parallel Execution

Prepare providers run in parallel, respecting the `jobs` setting for concurrency limits.
This speeds up preparation when multiple providers need to run (e.g., both npm and pip).

```toml
[settings]
jobs = 4  # Run up to 4 providers in parallel
```

## Example: Full-Stack Project

```toml
# mise.toml for a project with Node.js frontend and Python backend

[prepare.npm]
auto = true

[prepare.poetry]
auto = true

[prepare.prisma]
auto = true
sources = ["prisma/schema.prisma"]
outputs = ["node_modules/.prisma/"]
run = "npx prisma generate"

[prepare.frontend-codegen]
sources = ["schema.graphql", "codegen.ts"]
outputs = ["src/generated/"]
run = "npm run codegen"
```

Running `mise prep` will check all four providers and run any that are stale, in parallel.
