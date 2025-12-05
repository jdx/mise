## `aqua-tester`

Test Aqua configuration with fish shell


- **Usage**: `aqua-tester`

Test Aqua configuration with fish shell

## `build`

Build the project


- **Usage**: `build`
- **Aliases**: `b`

Build the project

## `ci`

Run all CI checks


- Depends: format, build, test

- **Usage**: `ci`

Run all CI checks

## `clean`

Clean build artifacts


- **Usage**: `clean`

Clean build artifacts

## `docs`

Start the documentation development server


- Depends: docs:setup

- **Usage**: `docs`

Start the documentation development server

## `docs:build`

Build the documentation site


- Depends: docs:setup

- **Usage**: `docs:build`

Build the documentation site

## `docs:demos`

Create recordings with vhs


- **Usage**: `docs:demos`

Create recordings with vhs

## `docs:preview`

Preview the documentation site


- Depends: docs:build

- **Usage**: `docs:preview`

Preview the documentation site

## `docs:release`

Release documentation site to production or remote


- Depends: docs:build

- **Usage**: `docs:release`

Release documentation site to production or remote

## `docs:setup`

Install documentation dependencies


- **Usage**: `docs:setup`

Install documentation dependencies

## `fetch-gpg-keys`

Fetch GPG keys for signing or verification


- **Usage**: `fetch-gpg-keys`

Fetch GPG keys for signing or verification

## `filetask`

This is a test build script


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

Generate a flamegraph for performance analysis


- **Usage**: `flamegraph`

Generate a flamegraph for performance analysis

## `install-dev`

Install the current project in debug mode


- **Usage**: `install-dev`

Install the current project in debug mode

## `lint`

Run all lint checks


- Depends: lint:*

- **Usage**: `lint`

Run all lint checks

## `lint-fix`

Automatically fix lint issues


- **Usage**: `lint-fix`
- **Aliases**: `format`, `fix`

Automatically fix lint issues

## `lint:actionlint`

Lint GitHub Actions workflows


- **Usage**: `lint:actionlint`

Lint GitHub Actions workflows

## `lint:cargo-fmt`

Check Rust code formatting with cargo fmt


- **Usage**: `lint:cargo-fmt`

Check Rust code formatting with cargo fmt

## `lint:hk`

Lint HK files


- **Usage**: `lint:hk`

Lint HK files

## `lint:markdownlint`

Lint Markdown files


- **Usage**: `lint:markdownlint`

Lint Markdown files

## `lint:ripgrep`

Lint using ripgrep


- **Usage**: `lint:ripgrep`

Lint using ripgrep

## `lint:schema`

Lint schemas


- **Usage**: `lint:schema`

Lint schemas

## `pre-commit`

Run pre-commit hooks


- **Usage**: `pre-commit`

Run pre-commit hooks

## `release`

Release the project


- **Usage**: `release`

Release the project

## `release-plz`

Release with release-plz


- **Usage**: `release-plz`

Release with release-plz

## `render`

Run all render tasks


- Depends: render:*

- **Usage**: `render`

Run all render tasks

## `render:completions`

Generate shell completions


- Depends: build

- **Usage**: `render:completions`

Generate shell completions

## `render:fig`

Generate Fig completion spec


- Depends: docs:setup

- **Usage**: `render:fig`

Generate Fig completion spec

## `render:help`

Render help documentation


- Depends: build

- **Usage**: `render:help`

Render help documentation

## `render:mangen`

Generate man pages


- Depends: render:usage

- **Usage**: `render:mangen`

Generate man pages

## `render:schema`

Render JSON schema


- Depends: docs:setup

- **Usage**: `render:schema`

Render JSON schema

## `render:usage`

Generate usage documentation


- Depends: build

- **Usage**: `render:usage`

Generate usage documentation

## `show-output-on-failure`

Show output on failure for documentation generation


- **Usage**: `show-output-on-failure`

Show output on failure for documentation generation

## `signal-test`

Test signal handling in Node.js


- **Usage**: `signal-test`

Test signal handling in Node.js

## `snapshots`

update test snapshots


- **Usage**: `snapshots`

update test snapshots

## `test`

run all tests


- **Usage**: `test`
- **Aliases**: `t`

run all tests

## `test:build-perf-workspace`

task description


- **Usage**: `test:build-perf-workspace`

task description

## `test:coverage`

Run all tests with coverage report


- **Usage**: `test:coverage`

Run all tests with coverage report

## `test:e2e`

Run end-to-end tests


- Depends: build

- **Usage**: `test:e2e`
- **Aliases**: `e`, `e2e`

Run end-to-end tests

## `test:perf`

Run performance tests


- Depends: test:build-perf-workspace

- **Usage**: `test:perf`

Run performance tests

## `test:shuffle`

Run tests with shuffling enabled


- **Usage**: `test:shuffle`

Run tests with shuffling enabled

## `test:unit`

run unit tests


- **Usage**: `test:unit`

run unit tests

## `update-descriptions`

Update all task descriptions in the project


- **Usage**: `update-descriptions`

Update all task descriptions in the project
