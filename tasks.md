## `aqua-tester`

- **Usage**: `aqua-tester`

Test Aqua configuration with fish shell

## `build`

- **Usage**: `build`
- **Aliases**: `b`

Build the project

## `ci`

- Depends: format, build, test

- **Usage**: `ci`

Run all CI checks

## `clean`

- **Usage**: `clean`

Clean build artifacts

## `docs`

- Depends: docs:setup

- **Usage**: `docs`

Start the documentation development server

## `docs:build`

- Depends: docs:setup

- **Usage**: `docs:build`

Build the documentation site

## `docs:demos`

- **Usage**: `docs:demos`

Create recordings with vhs

## `docs:preview`

- Depends: docs:build

- **Usage**: `docs:preview`

Preview the documentation site

## `docs:release`

- Depends: docs:build

- **Usage**: `docs:release`

Release documentation site to production or remote

## `docs:setup`

- **Usage**: `docs:setup`

Install documentation dependencies

## `fetch-gpg-keys`

- **Usage**: `fetch-gpg-keys`

Fetch GPG keys for signing or verification

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

## `flamegraph`

- **Usage**: `flamegraph`

Generate a flamegraph for performance analysis

## `install-dev`

- **Usage**: `install-dev`

Install the current project in debug mode

## `lint`

- Depends: lint:*

- **Usage**: `lint`

Run all lint checks

## `lint-fix`

- **Usage**: `lint-fix`
- **Aliases**: `format`, `fix`

Automatically fix lint issues

## `lint:actionlint`

- **Usage**: `lint:actionlint`

Lint GitHub Actions workflows

## `lint:cargo-fmt`

- **Usage**: `lint:cargo-fmt`

Check Rust code formatting with cargo fmt

## `lint:hk`

- **Usage**: `lint:hk`

Lint HK files

## `lint:markdownlint`

- **Usage**: `lint:markdownlint`

Lint Markdown files

## `lint:ripgrep`

- **Usage**: `lint:ripgrep`

Lint using ripgrep

## `lint:schema`

- **Usage**: `lint:schema`

Lint schemas

## `pre-commit`

- **Usage**: `pre-commit`

Run pre-commit hooks

## `release`

- **Usage**: `release`

Release the project

## `release-plz`

- **Usage**: `release-plz`

Release with release-plz

## `render`

- Depends: render:*

- **Usage**: `render`

Run all render tasks

## `render:completions`

- Depends: build

- **Usage**: `render:completions`

Generate shell completions

## `render:fig`

- Depends: docs:setup

- **Usage**: `render:fig`

Generate Fig completion spec

## `render:help`

- Depends: build

- **Usage**: `render:help`

Render help documentation

## `render:mangen`

- Depends: build

- **Usage**: `render:mangen`

Generate man pages

## `render:schema`

- Depends: docs:setup

- **Usage**: `render:schema`

Render JSON schema

## `render:usage`

- Depends: build

- **Usage**: `render:usage`

Generate usage documentation

## `show-output-on-failure`

- **Usage**: `show-output-on-failure`

Show output on failure for documentation generation

## `signal-test`

- **Usage**: `signal-test`

Test signal handling in Node.js

## `snapshots`

- **Usage**: `snapshots`

update test snapshots

## `test`

- **Usage**: `test`
- **Aliases**: `t`

run all tests

## `test:build-perf-workspace`

- **Usage**: `test:build-perf-workspace`

task description

## `test:coverage`

- **Usage**: `test:coverage`

Run all tests with coverage report

## `test:e2e`

- Depends: build

- **Usage**: `test:e2e`
- **Aliases**: `e`, `e2e`

Run end-to-end tests

## `test:perf`

- Depends: test:build-perf-workspace

- **Usage**: `test:perf`

Run performance tests

## `test:shuffle`

- **Usage**: `test:shuffle`

Run tests with shuffling enabled

## `test:unit`

- **Usage**: `test:unit`

run unit tests

## `update-descriptions`

- **Usage**: `update-descriptions`

Update all task descriptions in the project
