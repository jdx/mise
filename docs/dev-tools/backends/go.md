# Go Backend <Badge type="warning" text="experimental" />

You may install packages directly via [go install](https://go.dev/doc/install) even if there
isn't an asdf plugin for it.

The code for this is inside of the mise repository at [`./src/backend/go.rs`](https://github.com/jdx/mise/blob/main/src/backend/go.rs).

## Dependencies

This relies on having `go` installed. Which you can install via mise:

```sh
mise use -g go
```

::: tip
Any method of installing `go` is fine if you want to install go some other way.
mise will use whatever `go` is on PATH.
:::

## Usage

The following installs the latest version of [hivemind](https://github.com/DarthSim/hivemind) and
sets it as the active version on PATH:

```sh
$ mise use -g go:github.com/DarthSim/hivemind
$ hivemind --help
Hivemind version 1.1.0
```

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `go` backend—these
go in `[tools]` in `mise.toml`.

### `tags`

Specify go build tags (passed as `go install --tags`):

```toml
[tools]
"go:github.com/golang-migrate/migrate/v4/cmd/migrate" = { version = "latest", tags = "postgres" }
```
