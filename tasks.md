## `aqua-tester.fish`

- **Usage**: `aqua-tester.fish`

## `build`

- **Usage**: `build`
- **Aliases**: `b`

## `ci`

- Depends: format, build, test

- **Usage**: `ci`

## `clean`

- **Usage**: `clean`

## `docker:cargo`

- **Usage**: `docker:cargo`

run cargo inside of development docker container

## `docker:e2e`

- **Usage**: `docker:e2e`

run e2e tests inside of development docker container

## `docker:image`

- **Usage**: `docker:image`

build docker image from Dockerfile

## `docker:mise`

- **Usage**: `docker:mise`

run mise inside of development docker container

## `docker:run`

- Depends: docker:image

- **Usage**: `docker:run`

run a command inside of development docker container

## `docs`

- Depends: docs:setup

- **Usage**: `docs`

## `docs:build`

- Depends: docs:setup

- **Usage**: `docs:build`

## `docs:release`

- Depends: docs:build

- **Usage**: `docs:release`

## `docs:setup`

- **Usage**: `docs:setup`

## `fetch-gpg-keys`

- **Usage**: `fetch-gpg-keys`

## `filetask`

- **Usage**: `filetask [-f --force] [-u --user <user>] [file] [arg_with_default]`
- **Aliases**: `ft`

This is a test build script

### Arguments

#### `[file]`

The file to write

**Default:** `file.txt`

#### `[arg_with_default]`

An arg with a default

**Default:** `mydefault`

### Flags

#### `-f --force`

Overwrite existing &lt;file>

#### `-u --user <user>`

User to run as

## `filetask.bat`

- **Usage**: `filetask.bat`

## `install-dev`

- **Usage**: `install-dev`

## `lint`

- Depends: lint:*

- **Usage**: `lint`

## `lint-fix`

- **Usage**: `lint-fix`
- **Aliases**: `format`

## `lint:actionlint`

- **Usage**: `lint:actionlint`

## `lint:cargo-fmt`

- **Usage**: `lint:cargo-fmt`

## `lint:clippy`

- **Usage**: `lint:clippy`

## `lint:markdownlint`

- **Usage**: `lint:markdownlint`

## `lint:prettier`

- **Usage**: `lint:prettier`

## `lint:ripgrep`

- **Usage**: `lint:ripgrep`

## `lint:shellcheck`

- **Usage**: `lint:shellcheck`

## `lint:shfmt`

- **Usage**: `lint:shfmt`

## `lint:toml`

- **Usage**: `lint:toml`

## `pre-commit`

- Depends: render, lint

- **Usage**: `pre-commit`

## `release`

- **Usage**: `release`

## `release-plz`

- **Usage**: `release-plz`

## `render`

- Depends: render:*

- **Usage**: `render`
- **Aliases**: `render`

## `render:completions`

- Depends: build

- **Usage**: `render:completions`

## `render:fig`

- Depends: docs:setup

- **Usage**: `render:fig`

## `render:help`

- Depends: build

- **Usage**: `render:help`

## `render:mangen`

- Depends: build

- **Usage**: `render:mangen`

## `render:registry`

- Depends: build

- **Usage**: `render:registry`

## `render:settings`

- Depends: docs:setup

- **Usage**: `render:settings`

## `render:usage`

- Depends: build

- **Usage**: `render:usage`

## `show-output-on-failure`

- **Usage**: `show-output-on-failure`

## `signal-test`

- **Usage**: `signal-test`

## `snapshots`

- **Usage**: `snapshots`

update test snapshots

## `test`

- **Usage**: `test`
- **Aliases**: `t`

run all tests

## `test:coverage`

- **Usage**: `test:coverage`

run all tests with coverage report

## `test:e2e`

- Depends: build

- **Usage**: `test:e2e`
- **Aliases**: `e`

run end-to-end tests

## `test:shuffle`

- **Usage**: `test:shuffle`

## `test:unit`

- **Usage**: `test:unit`

run unit tests

## `update-descriptions`

- **Usage**: `update-descriptions`

## `xxx`

- **Usage**: `xxx`

a task for testing
