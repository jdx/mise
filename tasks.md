## ``

- **Usage**: ``

Test Aqua configuration with fish shell

## ``

- **Usage**: ``
- **Aliases**: `b`

Build the project

## ``

- Depends: format, build, test

- **Usage**: ``

Run all CI checks

## ``

- **Usage**: ``

Clean build artifacts

## ``

- Depends: docs:setup

- **Usage**: ``

Start the documentation development server

## ``

- Depends: docs:setup

- **Usage**: ``

Build the documentation site

## ``

- **Usage**: ``

Create recordings with vhs

## ``

- Depends: docs:build

- **Usage**: ``

Preview the documentation site

## ``

- Depends: docs:build

- **Usage**: ``

Release documentation site to production or remote

## ``

- **Usage**: ``

Install documentation dependencies

## ``

- **Usage**: ``

Fetch GPG keys for signing or verification

## ``

- **Usage**: `[-f --force] [-u --user <user>] [file] [arg_with_default]`
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

## ``

- **Usage**: ``

## ``

- **Usage**: ``

Generate a flamegraph for performance analysis

## ``

- **Usage**: ``

Install the current project in debug mode

## ``

- Depends: lint:*

- **Usage**: ``

Run all lint checks

## ``

- **Usage**: ``
- **Aliases**: `format`, `fix`

Automatically fix lint issues

## ``

- **Usage**: ``

Lint GitHub Actions workflows

## ``

- **Usage**: ``

Check Rust code formatting with cargo fmt

## ``

- **Usage**: ``

Lint HK files

## ``

- **Usage**: ``

Lint Markdown files

## ``

- **Usage**: ``

Lint using ripgrep

## ``

- **Usage**: ``

Lint schemas

## ``

- **Usage**: ``

Run pre-commit hooks

## ``

- **Usage**: ``

Release the project

## ``

- **Usage**: ``

Release with release-plz

## ``

- Depends: render:*

- **Usage**: ``

Run all render tasks

## ``

- Depends: build

- **Usage**: ``

Generate shell completions

## ``

- Depends: docs:setup

- **Usage**: ``

Generate Fig completion spec

## ``

- Depends: build

- **Usage**: ``

Render help documentation

## ``

- Depends: build

- **Usage**: ``

Generate man pages

## ``

- Depends: docs:setup

- **Usage**: ``

Render JSON schema

## ``

- Depends: build

- **Usage**: ``

Generate usage documentation

## ``

- **Usage**: ``

Show output on failure for documentation generation

## ``

- **Usage**: ``

Test signal handling in Node.js

## ``

- **Usage**: ``

update test snapshots

## ``

- **Usage**: ``
- **Aliases**: `t`

run all tests

## ``

- **Usage**: ``

task description

## ``

- **Usage**: ``

Run all tests with coverage report

## ``

- Depends: build

- **Usage**: ``
- **Aliases**: `e`, `e2e`

Run end-to-end tests

## ``

- Depends: test:build-perf-workspace

- **Usage**: ``

Run performance tests

## ``

- **Usage**: ``

Run tests with shuffling enabled

## ``

- **Usage**: ``

run unit tests

## ``

- **Usage**: ``

Update all task descriptions in the project
