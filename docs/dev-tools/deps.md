---
description: Run your project's package installers, such as npm install or uv sync, when their lockfiles change or their outputs go missing.
socialDescription: Install project packages when lockfiles change or outputs go missing.
---

# Project dependencies <Badge type="warning" text="experimental" />

`mise deps` installs a project's packages, such as `node_modules` or a Python
virtual environment, by running the package manager's install command when the
files it tracks change or its outputs go missing. Use `[tools]` to install the
package manager itself and `[deps]` to install the project's packages.

::: warning Experimental
`mise deps` is experimental. Enable it with `experimental = true` under
`[settings]`, as in the examples below, or with `MISE_EXPERIMENTAL=1`.
:::

## Quick start

For an npm project with `package.json` and `package-lock.json`, add:

```toml [mise.toml]
[settings]
experimental = true

[tools]
node = "24"

[deps.npm]
auto = true
```

Install Node.js, check that mise sees the provider, install the packages, and
ask why the provider is now fresh:

```sh
mise install
mise deps install --list
mise deps install npm
mise deps install npm --explain
```

With `auto = true`, `mise exec` and `mise run` also run the provider first
whenever it is stale. A provider stays inactive until its input files exist, so
if the project has no lockfile yet, create one with the package manager first,
for example `mise exec --no-deps -- npm install`.

## Enable and disable providers {#configuration}

Configure only the providers your project uses; mise does not detect them on its
own. An empty table enables a built-in provider without making it automatic:

```toml
[deps.uv]
```

To turn off a provider, for example one inherited from another config file:

```toml
[deps]
disable = ["npm"]
```

This stops the provider from running; it does not remove installed packages.
Run `mise deps install --list` to see the effective providers and whether each
is active.

## Built-in providers

Each built-in provider supplies default sources, outputs, and an install
command:

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
| `uv`            | `pyproject.toml`, `uv.lock`                            | Optional `.venv/`                | `uv sync`                                                        |
| `bundler`       | `Gemfile`, `Gemfile.lock`                              | Optional `vendor/bundle/`        | `bundle install`                                                 |
| `composer`      | `composer.json`, `composer.lock`                       | `vendor/`                        | `composer install`                                               |
| `dart`          | `pubspec.yaml`, `pubspec.lock`                         | `.dart_tool/package_config.json` | `dart pub get`                                                   |
| `flutter`       | `pubspec.yaml`, `pubspec.lock`                         | `.dart_tool/package_config.json` | `flutter pub get`                                                |
| `git-submodule` | `.gitmodules`                                          | Declared submodule directories   | `git submodule update --init --recursive`                        |

A provider is active only when its required project input exists. Most need
their lockfile; the exceptions are `go` (`go.mod`), `pip` (`requirements.txt`),
`dart` and `flutter` (`pubspec.yaml`), and `git-submodule` (a non-empty
`.gitmodules`). Members of a pub workspace track the workspace's package
configuration file.

mise checks an optional output for deletion only after it has seen that output
following a successful run. This supports package managers that install outside
the project by default. The `pip` provider does not create or select a virtual
environment: set up [Python virtualenv activation](/lang/python.html#automatic-virtualenv-activation)
first if pip should install into `.venv`.

The default commands are ordinary installs, not necessarily frozen-lockfile
installs. To require npm's clean install from the lockfile, override `run`:

```toml
[deps.npm]
run = "npm ci"
```

The freshness check still decides whether the command runs. Pass `--force` to
run it even when nothing it tracks has changed.

## Custom providers

Define a provider for any project-specific step that turns input files into
outputs. These examples assume `@graphql-codegen/cli` and `prisma` are already
project dependencies and that their scripts and configuration exist:

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

### Provider options

These options apply to custom and built-in providers:

| Option        | Type     | Description                                                                |
| ------------- | -------- | -------------------------------------------------------------------------- |
| `auto`        | bool     | Run before `mise exec` and `mise run` when stale (default: `false`).       |
| `sources`     | string[] | Files or glob patterns whose contents decide freshness.                    |
| `outputs`     | string[] | Files or directories that must exist for the provider to be fresh.         |
| `run`         | string   | Command to run when the provider is stale.                                 |
| `env`         | table    | Environment variables for the command.                                     |
| `dir`         | string   | Base directory for sources, outputs, and the command.                      |
| `description` | string   | Description shown in output.                                               |
| `depends`     | string[] | Providers that must finish successfully first.                             |
| `timeout`     | string   | Time limit for the command, such as `"30s"` or `"5m"` (default: no limit). |

On a built-in provider, setting `sources` or `outputs` replaces that provider's
defaults instead of adding to them. An empty array, such as `outputs = []`,
turns off that kind of path tracking, including any optional outputs the
built-in provider supplies.

Relative paths and glob patterns resolve from the provider's config root after
applying `dir`; absolute paths are used as written. For example, a pnpm
workspace that keeps installed packages under an application directory can
override the root-level defaults:

```toml
[deps.pnpm]
sources = ["pnpm-lock.yaml", "packages/app/package.json"]
outputs = ["packages/app/node_modules"]
```

### Templates and environment variables

String values in provider configuration accept [Tera templates](/templates.html)
such as <span v-pre>`{{ config_root }}`</span>,
<span v-pre>`{{ env.NAME }}`</span>, and <span v-pre>`{{ vars.name }}`</span>.
Shell-style variables such as `$NAME` and `${NAME:-default}` are expanded after
the templates, following the same
[`env_shell_expand`](/configuration/settings.html#env_shell_expand) setting as
`[env]` values.

```toml
[vars]
package = "api"

[deps.codegen]
sources = ["schemas/$SCHEMA_NAME.graphql"]
outputs = ["generated/${SCHEMA_NAME:-default}/"]
env = { OUTPUT_PACKAGE = "{{ vars.package }}" }
run = 'npm run codegen -- "$OUTPUT_PACKAGE"'
```

`$SCHEMA_NAME` comes from your environment, and `${SCHEMA_NAME:-default}` falls
back to `default` when it is unset. An undefined variable without a default is
left as written, with a warning; use `${NAME:-}` to default it to an empty
string.

In `run`, Tera expressions are rendered when the configuration loads, but `$VAR`
expressions are left for the provider's shell to expand when the command runs,
so `run` can use values from the provider's `env` table. Quote shell expansions
that should stay one argument. Provider IDs and environment variable names are
not templated. An invalid Tera template is reported as a configuration error
before any provider command starts.

## Order and parallelism

Providers without `depends` run in parallel, up to the
[`jobs`](/configuration/settings.html#jobs) setting. A provider with `depends`
waits until those providers succeed. If one fails, the providers that depend on
it are skipped. A dependency cycle is reported with a warning, and the providers
in it are skipped.

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

This assumes the uv project declares `ansible-core` and that its virtual
environment is on `PATH`, for example through `_.python.venv`. The
`ansible-galaxy` provider starts after `uv` finishes. `depends` only orders
configured providers; it does not enable a provider or install a package
manager.

## Run providers automatically {#auto-install}

A provider with `auto = true` runs, when it is stale, before:

- [`mise run`](/cli/run.html) runs a task
- [`mise exec`](/cli/exec.html) runs a command

These automatic runs use the same sources and outputs as `mise deps`. They
handle tracked changes before your command starts; they do not upgrade packages
to newer upstream versions. To skip them for one invocation:

```sh
mise run --no-deps build
mise exec --no-deps -- npm test
```

### Staleness warnings

In an [activated shell](/shell-setup.html), mise warns when an `auto = true`
provider is stale, naming each provider and the reason:

```text
mise WARN  deps: npm (package-lock.json changed) — run `mise deps`
```

To turn the warning off, set
[`status.show_deps_stale`](/configuration/settings.html#status.show_deps_stale)
to `false`:

```toml
[settings]
status.show_deps_stale = false
```

## How freshness is checked {#freshness-checking}

A provider is fresh when its required outputs exist and nothing it tracks has
changed since its last successful run: the contents of its `sources`, and its
effective command (the `run` string, shell, `env`, and `dir`). Otherwise it is
stale and mise runs it.

- A provider with sources is stale on its first run.
- An optional output must keep existing once mise has seen it.
- A custom provider with outputs but no sources is fresh while its outputs
  exist; editing its command alone does not rerun it.
- A provider with neither sources nor outputs runs every time.

mise does not check installed packages one by one, look for newer upstream
releases, or notice when an untracked external package cache is removed. List
the input files a command's result depends on in `sources`. It keeps this state
in `$MISE_STATE_DIR/deps/`, not in the project, and stores hashes, not the
command or environment values.

To see the decision, or what would run, use:

```sh
mise deps install npm --explain
mise deps install --dry-run
```

`--explain` exits 0 only when the provider is fresh. It exits non-zero when the
provider is stale or inactive (for example, its lockfile is missing), so it can
serve as a CI check. To run a provider whose files changed outside the tracked
state, pass `--force`.

## Monorepos

By default, `mise deps` runs only the providers defined in the current project's
config files, plus any in your global config. To run the providers of every
explicitly configured monorepo root, pass `--monorepo`:

```toml
monorepo_root = true

[monorepo]
config_roots = ["apps/*", "packages/*"]
```

```sh
mise deps --monorepo
```

This requires explicit
[`[monorepo].config_roots`](/tasks/monorepo.html#explicit-config-roots); mise
does not search arbitrary subdirectories for providers. Providers in the
monorepo root's config are included too, because that config is part of every
selected config root's hierarchy, as with `mise install --monorepo`.

Monorepo provider IDs include their config root, so the same provider can
appear in several projects. For example, two uv providers are named
`//apps/api:uv` and `//apps/worker:uv`. Use the qualified name with `--only`,
`--skip`, or the provider argument:

```sh
mise deps --monorepo --only //apps/api:uv
mise deps install //apps/worker:uv --monorepo
```

A `depends` entry without a `//` prefix resolves within the same config root, so
a provider in `apps/api` with `depends = ["uv"]` depends on `//apps/api:uv`.

When `mise run` runs a task from another config root, such as
`mise run //apps/api:build`, the `auto = true` providers of that config root run
first, along with those of the project you run it from (the monorepo root when
you run it there) and your global config.

For a single nested project, the `dir` option is simpler:

```toml
[deps.uv]
dir = "apps/api"
```

## Add and remove packages {#adding-and-removing-packages}

`mise deps add` and `mise deps remove` run a package manager's add and remove
commands, which update the project's manifest, such as `package.json`, and its
lockfile. Name each package as `ecosystem:package`:

```sh
mise deps add npm:react
mise deps add npm:@types/react@19
mise deps add -D npm:vitest        # dev dependency
mise deps remove npm:lodash
```

These commands support `npm`, `yarn`, `pnpm`, `bun`, `deno`, `aube`, `dart`, and
`flutter`. They install missing tools from `[tools]` first, and they do not need
a `[deps]` entry, but they use the provider's `dir` when one is configured. For
the other providers, run the package manager directly, for example
`mise exec -- uv add httpx`.

## Full-stack example {#example-full-stack-project}

This example assumes a repository with npm and uv projects at its root, both
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

`mise deps` runs stale `npm` and `uv` providers in parallel. `prisma` and
`frontend-codegen` wait for `npm` and can then run in parallel with each other.
`frontend-codegen` has no `auto = true`, so it runs only when you run
`mise deps`, not before every `mise exec` or task.

For every flag, see [`mise deps`](/cli/deps.html),
[`mise deps install`](/cli/deps/install.html),
[`mise deps add`](/cli/deps/add.html), and
[`mise deps remove`](/cli/deps/remove.html).
