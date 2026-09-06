# Deps <Badge type="warning" text="experimental" />

`mise deps` runs project dependency installers when tracked inputs change or
outputs go missing. It compares source hashes with the last successful run, then
invokes the configured package manager. Use `[tools]` to install the package
manager itself; use `[deps]` to install the project's packages.

## Quick Start

For an existing npm project with `package.json` and `package-lock.json`, add:

```toml [mise.toml]
[settings]
experimental = true

[tools]
node = "24"

[deps.npm]
auto = true
```

Inspect the provider, install its dependencies, and explain the freshness result:

```sh
mise install
mise deps install --list
mise deps install npm
mise deps install npm --explain
```

With `auto = true`, subsequent `mise exec` and `mise run` commands also check
this provider. If the project has no lockfile yet, create one with its package
manager first, for example `mise exec --no-deps -- npm install`.

## Configuration

Enable only the providers your project uses. An empty table selects a built-in
provider without making it automatic:

```toml
[deps.uv]
```

To disable a provider, for example one inherited from another configuration:

```toml
[deps]
disable = ["npm"]
```

This prevents the provider from running; it does not remove installed packages.
Use `mise deps install --list` to inspect the effective providers.

## Built-in Providers

Each provider supplies defaults for sources, outputs, and the install command:

| Provider        | Sources                                                | Tracked output                   | Default command                                                  |
| --------------- | ------------------------------------------------------ | -------------------------------- | ---------------------------------------------------------------- |
| `npm`           | `package.json`, `package-lock.json`                    | `node_modules/`                  | `npm install`                                                    |
| `yarn`          | `package.json`, `yarn.lock`                            | `node_modules/`                  | `yarn install`                                                   |
| `pnpm`          | `package.json`, `pnpm-lock.yaml`                       | `node_modules/`                  | `pnpm install`                                                   |
| `bun`           | `package.json`, `bun.lock` or `bun.lockb`              | `node_modules/`                  | `bun install`                                                    |
| `deno`          | `deno.json`, `deno.jsonc`, `package.json`, `deno.lock` | Optional `node_modules/`         | `deno install`                                                   |
| `aube`          | `package.json`, `aube-lock.yaml`                       | `node_modules/`                  | `aube install`                                                   |
| `go`            | `go.mod`, `go.sum`                                     | Optional `vendor/`               | `go mod vendor` if `vendor/` exists, otherwise `go mod download` |
| `pip`           | `requirements.txt`                                     | Optional `.venv/`                | `pip install -r requirements.txt`                                |
| `poetry`        | `pyproject.toml`, `poetry.lock`                        | Optional `.venv/`                | `poetry install`                                                 |
| `uv`            | `pyproject.toml`, `uv.lock`                            | `.venv/`                         | `uv sync`                                                        |
| `bundler`       | `Gemfile`, `Gemfile.lock`                              | Optional `vendor/bundle/`        | `bundle install`                                                 |
| `composer`      | `composer.json`, `composer.lock`                       | `vendor/`                        | `composer install`                                               |
| `dart`          | `pubspec.yaml`, `pubspec.lock`                         | `.dart_tool/package_config.json` | `dart pub get`                                                   |
| `flutter`       | `pubspec.yaml`, `pubspec.lock`                         | `.dart_tool/package_config.json` | `flutter pub get`                                                |
| `git-submodule` | `.gitmodules`                                          | Declared submodule directories   | `git submodule update --init --recursive`                        |

Providers must be configured explicitly and have the required project input.
Most require their lockfile. The exceptions are `go` (`go.mod`), `pip`
(`requirements.txt`), Dart/Flutter (`pubspec.yaml`), and `git-submodule`
(a nonempty `.gitmodules`). Pub workspace members track the workspace's package
configuration file.

An **optional output** is checked for deletion only after mise has observed it
following a successful run. This supports package managers that install outside
the project by default. In particular, the pip provider does not create or select
a virtualenv: configure [Python virtualenv activation](/lang/python.html#automatic-virtualenv-activation)
first if pip should install into `.venv`.

These defaults are ordinary install commands, not necessarily frozen-lockfile
installs. To require npm's clean, lockfile-based installation, override `run`:

```toml
[deps.npm]
run = "npm ci"
```

The freshness check still decides whether to run it. Use `--force` when you need
the command to execute even if its tracked state is unchanged.

## Monorepos

By default, `mise deps` only runs providers from the current config root. To run
providers from every explicitly configured monorepo root, use `--monorepo`:

```toml
monorepo_root = true

[monorepo]
config_roots = ["apps/*", "packages/*"]
```

```bash
mise deps --monorepo
```

This requires explicit [`[monorepo].config_roots`](/tasks/monorepo.html#config-roots);
mise does not search arbitrary subdirectories for dependency providers.
Providers in the monorepo root config are also included because that config is
part of every selected config root's hierarchy, matching the behavior of
`mise install --monorepo`.

Monorepo provider IDs include their config root so the same provider can appear
in multiple projects. For example, two uv providers are named `//apps/api:uv`
and `//apps/worker:uv`. Use the qualified name with `--only`, `--skip`, or the
positional provider argument:

```bash
mise deps --monorepo --only //apps/api:uv
mise deps install //apps/worker:uv --monorepo
```

Provider dependencies without a `//` prefix are resolved within the same config
root. A provider in `apps/api` with `depends = ["uv"]` therefore depends on
`//apps/api:uv`.

For a single nested project, the `dir` option remains a simpler alternative:

```toml
[deps.uv]
dir = "apps/api"
```

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

The ecosystem prefix tells mise which package manager to use. The ecosystems currently
supported for add/remove are `npm`, `yarn`, `pnpm`, `bun`, `deno`, `aube`, `dart`, `flutter`.

## Custom Providers

Create custom providers for project-specific build steps. These examples assume
`@graphql-codegen/cli` and `prisma` are already project dependencies and the
corresponding scripts/configuration exist:

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
| `dir`         | string   | Base directory for sources, outputs, and the command                      |
| `description` | string   | Description shown in output                                               |
| `depends`     | string[] | Other provider names that must complete before this one runs              |
| `timeout`     | string   | Timeout for the run command, e.g., `"30s"`, `"5m"` (default: no timeout)  |

Built-in providers use their documented sources and outputs when these options are
omitted. Setting `sources` or `outputs` replaces that provider's defaults rather
than adding to them. An empty array, such as `outputs = []`, explicitly disables
that kind of path tracking; it also disables any optional outputs supplied by the
built-in provider.

Relative paths and glob patterns are resolved from the provider's config root after
applying `dir`. Absolute paths are used as written. For example, a pnpm workspace
that keeps installed packages below an application directory can override the
root-level defaults:

```toml
[deps.pnpm]
sources = ["pnpm-lock.yaml", "packages/app/package.json"]
outputs = ["packages/app/node_modules"]
```

### Templates and Environment Variables

String values in provider configuration support Tera templates such as
<span v-pre>`{{ config_root }}`</span>, <span v-pre>`{{ env.NAME }}`</span>, and
<span v-pre>`{{ vars.name }}`</span>. Shell-style environment variables
such as `$NAME` and `${NAME:-default}` are expanded after Tera templates, using the same
`env_shell_expand` setting as `[env]` values.

```toml
[vars]
package = "api"

[deps.codegen]
sources = ["{{ config_root }}/schemas/$SCHEMA_NAME.graphql"]
outputs = ["{{ config_root }}/generated/${SCHEMA_NAME:-default}/"]
dir = "{{ config_root }}"
env = { OUTPUT_PACKAGE = "{{ vars.package }}-$BUILD_MODE" }
run = 'npm run codegen -- "$OUTPUT_PACKAGE"'
```

Set `SCHEMA_NAME` and `BUILD_MODE` in the environment before running this example.
Quote shell expansions in `run` when a value should remain one argument.

`$VAR` expressions in `run` are left for the provider's shell to expand at execution time. This
allows `run` to use values from the provider's `env` table. Tera expressions in `run` are rendered
when the provider configuration is loaded.

Provider IDs and environment-variable names are not templated. Invalid Tera templates are reported
as configuration errors before a provider command starts. An undefined shell-style variable is left
unchanged with a warning; use `${NAME:-}` to explicitly default it to an empty string.

## Freshness Checking

mise uses blake3 hashing to determine whether sources or the effective provider command have
changed since the last successful run. Hashes are stored in
`$MISE_STATE_DIR/deps/<hash>.toml`, keyed by project root (so nothing is written inside
the project directory). Command hashes include the run command, shell, provider `env`,
and working directory; raw command and environment values are not stored in state.

1. Compute blake3 hashes of all source files
2. Compute a blake3 hash of the effective provider command
3. Compare against stored hashes from the last successful run
4. Mark the provider stale if a source or the effective command was added, removed, or changed

Required outputs must exist. Optional outputs must continue to exist once they
have been observed. With source tracking, the first run is stale and changes to
sources or the effective command trigger another run.

For custom providers with no sources, existing outputs are enough to be fresh;
command changes alone do not invalidate them. A provider with neither sources
nor outputs runs every time. Configure real input files when a command's result
depends on their contents.

Freshness checks do not inspect every installed package, query for newer upstream
releases, or detect removal of an untracked external package cache. Use
`mise deps install <provider> --explain` to see the decision, and `--force` to
repair dependencies whose files changed outside the tracked state.

State created before command hashing is migrated by running source-tracked
providers once.

## Auto-Install

When `auto = true` is set on a provider, it runs automatically before:

- `mise run` (task execution)
- `mise x` (exec command)

Automatic checks use the same sources and outputs as `mise deps`; they ensure
tracked changes are handled before execution. They do not upgrade packages to
the newest upstream versions.

To skip auto-install for a single invocation:

```bash
mise run --no-deps build
mise x --no-deps -- npm test
```

## Staleness Warnings

When using `mise activate`, mise warns you if any auto-enabled providers have stale dependencies:

```
mise WARN deps: npm may need update, run `mise deps`
```

Disable this with:

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
waits for all of its dependencies to complete successfully before running.

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

This assumes the uv project declares `ansible-core` and its virtualenv is on
`PATH` (for example through `_.python.venv`). The `ansible-galaxy` provider waits
for `uv` to finish before starting. A `depends` entry orders configured providers;
it does not declare a missing provider or install a package manager.

Providers without `depends` run in parallel. If a dependency fails, all providers
that depend on it are skipped. Circular dependencies are detected and the affected providers
are skipped with a warning.

## Parallel Execution

Deps providers run in parallel, respecting the `jobs` setting for concurrency limits.
This speeds up installation when multiple providers need to run (e.g., both npm and pip).
Providers with `depends` wait for their dependencies to complete before starting,
while independent providers run concurrently.

```toml
[settings]
jobs = 4  # Run up to 4 providers in parallel
```

## Example: Full-Stack Project

This example assumes a repository with npm and uv projects in its root, both
lockfiles committed, Prisma installed as a project dependency, and an npm
`codegen` script:

```toml [mise.toml]
[settings]
experimental = true

[tools]
node = "24"
python = "3.14"
uv = "latest"

[deps.npm]
auto = true

[deps.uv]
auto = true

[deps.prisma]
auto = true
depends = ["npm"]
sources = ["prisma/schema.prisma", "package-lock.json"]
outputs = ["node_modules/.prisma/"]
run = "npx --no-install prisma generate"

[deps.frontend-codegen]
depends = ["npm"]
sources = ["schema.graphql", "codegen.ts", "package-lock.json"]
outputs = ["src/generated/"]
run = "npm run codegen"
```

`mise deps` runs stale npm and uv providers in parallel. Prisma and frontend
codegen wait for npm, then can run in parallel with each other. The codegen
provider has no `auto = true`, so it runs through explicit `mise deps` commands,
not automatically before every `mise exec` or task invocation.
