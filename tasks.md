## `a1 `

## `a2 `

## `b1 `

* Depends: a1, a2

## `build `

## `c1 `

* Depends: b1

## `ci `

* Depends: format, build, test

## `clean `

## `docker:cargo `

run cargo inside of development docker container

## `docker:e2e `

run e2e tests inside of development docker container

## `docker:image `

build docker image from Dockerfile

## `docker:mise `

run mise inside of development docker container

## `docker:run `

* Depends: docker:image

run a command inside of development docker container

## `filetask [args] [flags]`

* Depends: lint, build

This is a test build script

### Arguments

#### `<file>`

The file to write

#### `<arg_with_default>`

An arg with a default

### Flags

#### `-f --force`

Overwrite existing &lt;file>

#### `-u --user <user>`

User to run as

## `l `

## `l `

## `lint `

* Depends: lint:*

## `lint-fix `

## `lint:actionlint `

## `lint:cargo-fmt `

## `lint:clippy `

## `lint:markdownlint `

## `lint:prettier `

## `lint:ripgrep `

## `lint:settings `

## `lint:shellcheck `

## `lint:shfmt `

## `pre-commit `

* Depends: render, lint

## `release `

## `release-docs `

## `release-plz `

## `render `

* Depends: render:*

## `render:completions `

* Depends: build, render:usage

## `render:help `

* Depends: build

## `render:mangen `

* Depends: build

## `render:registry `

* Depends: build

## `render:settings `

## `render:usage `

* Depends: build

## `show-output-on-failure `

## `signal-test `

## `snapshots `

update test snapshots

## `test `

run all tests

## `test:coverage `

run all tests with coverage report

## `test:e2e `

* Depends: build

run end-to-end tests

## `test:shuffle `

## `test:unit `

run unit tests

## `update-shorthand-repo `
