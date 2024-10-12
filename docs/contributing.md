# Contributing

Before submitting a PR, unless it's something obvious, consider filing an issue or simply mention what you plan to do in the [Discord](https://discord.gg/UBa7pJUN7Z).
PRs are often either rejected or need to change significantly after submission so make sure before you start working on something it won't be a wasted effort.

## Dev Container

There is a docker setup that makes development with mise easier. It is especially helpful for running the E2E tests.
Here's some example ways to use it:

```sh
mise run docker:cargo build
mise run docker:cargo test
mise run docker:mise --help # run `mise --help` in the dev container
mise run docker:mise run test:e2e # run the e2e tests inside of the docker container
mise run docker:e2e # shortcut for `mise run docker:mise run test:e2e`
```

## Testing

### Unit Tests

Unit tests are used for fast, relatively simple logic. They can be run with `cargo test`. The dev container is recommended
for executing since it does not require having a proper local setup.

To run locally you will need to first disable mise if you are using it.

:::code-group

```sh [dev container]
mise run docker:cargo test
```

```sh [local]
cargo test
```

:::

### E2E Tests

Like unit tests, the e2e tests should be run either in the dev container (recommended) or with mise disabled locally.

:::code-group

```sh [dev container]
mise run docker:e2e
```

```sh [local]
mise run test:e2e
```

:::

Slow tests do not run by default or on PRs. They can be manually enabled with `TEST_ALL=1`.

## Dependencies

- [rust](https://www.rust-lang.org/) stable 1.70.0+ (I test with the beta channel locally, but CI uses stable, you can use whatever)
- [just](https://github.com/casey/just) this should be removed in favor of mise tasks but it's still used for some things.

## Tasks

Mise uses mise itself to run tasks. See available tasks with `mise tasks`:

```sh
~/src/mise ❯ mise tasks
build                                                        ~/src/mise/.mise.toml
clean                                                        ~/src/mise/.mise.toml
docker:cargo        run cargo inside of development docker … ~/src/mise/.mise.toml
docker:e2e          run e2e tests inside of development doc… ~/src/mise/.mise.toml
docker:image        build docker image from Dockerfile       ~/src/mise/.mise.toml
docker:mise         run mise inside of development docker c… ~/src/mise/.mise.toml
format                                                       ~/src/mise/.mise.toml
lint                                                         ~/src/mise/.mise/config.toml
lint:fix                                                     ~/src/mise/.mise.toml
release                                                      ~/src/mise/.mise.toml
render                                                       ~/src/mise/.mise.toml
render:completions                                           ~/src/mise/.mise.toml
render:help                                                  ~/src/mise/.mise.toml
render:mangen                                                ~/src/mise/.mise.toml
signal-test                                                  ~/src/mise/.mise.toml
snapshots           Update test snapshots                    ~/src/mise/.mise.toml
test                                                         ~/src/mise/.mise.toml
test:e2e                                                     ~/src/mise/.mise.toml
```

## Setup

Shouldn't require anything special I'm aware of, but `mise run build` is a good sanity check to run and make sure it's all working.

## Pre-commit hook

You can optionally run a pre-commit hook which lints the codebase and updates generated code.
To do this, install [lefthook](https://github.com/evilmartians/lefthook) and run `lefthook install`.

## Running the CLI

Even if using the devcontainer, it's a good idea to create a shim to make it easy to launch mise. I use the following shim
in `~/.local/bin/@mise`:

```sh
#!/bin/sh
exec cargo run -q --all-features --manifest-path ~/src/mise/Cargo.toml -- "$@"
```

::: note
Don't forget to change the manifest path to the correct path for your setup.
:::

Then if that is in PATH just use `@mise` to run mise by compiling it on the fly.

```sh
@mise --help
@mise run docker:e2e
eval "$(@mise activate zsh)"
@mise activate fish | source
```

## Releasing

Run `mise run release -x [minor|patch]`. (minor if it is the first release in a month)

## Linting

- Lint codebase: `mise run lint`
- Lint and fix codebase: `mise run lint:fix`

## Generating readme and shell completion files

```sh
mise run render
```

## Adding a new setting

To add a new setting, add it to [`settings.toml`](https://github.com/jdx/mise/blob/main/settings.toml) in the root of the project and run `mise run render` to update the codebase.

## Testing packaging

This is only necessary to test if actually changing the packaging setup.

### Ubuntu (apt)

This is for arm64, but you can change the arch to amd64 if you want.

```sh
docker run -ti --rm ubuntu
apt update -y
apt install -y gpg sudo wget curl
sudo install -dm 755 /etc/apt/keyrings
wget -qO - https://mise.jdx.dev/gpg-key.pub | gpg --dearmor | sudo tee /etc/apt/keyrings/mise-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/etc/apt/keyrings/mise-archive-keyring.gpg arch=arm64] https://mise.jdx.dev/deb stable main" | sudo tee /etc/apt/sources.list.d/mise.list
apt update
apt install -y mise
mise -V
```

### Amazon Linux 2 (yum)

```sh
docker run -ti --rm amazonlinux
yum install -y yum-utils
yum-config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
yum install -y mise
mise -v
```

### Fedora (dnf)

```sh
docker run -ti --rm fedora
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
dnf install -y mise
mise -v
```
