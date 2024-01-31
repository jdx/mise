# Contributing

Before submitting a PR, unless it's something obvious, consider filing an issue or simply mention what you plan to do in the [Discord](https://discord.gg/UBa7pJUN7Z).
PRs are often either rejected or need to change significantly after submission so make sure before you start working on something it won't be a wasted effort.

Again, please **reach out** first. People almost never do and submit PRs out of nowhere. I would much prefer you let me
know what you're working on before you start.

## Dev Container

There is a docker setup that makes development with mise easier. It is especially helpful for running the E2E tests.
Here's some example ways to use it:

```sh
$ mise run docker:cargo build
$ mise run docker:cargo test
$ mise run docker:mise --help # run `mise --help` in the dev container
$ mise run docker:mise run test:e2e # run the e2e tests inside of the docker container
$ mise run docker:e2e # shortcut for `mise run docker:mise run test:e2e`
```

> ![NOTE]
> There used to be a "devcontainer" that was unrelated to this functionality which was compatible with vscode and
github codespaces. I removed this to avoid confusion with the new, simpler dev container that makes use of mise tasks.
I'd accept a pr to make the current dev container compatible with vscode and codespaces or something similar.

## Testing

### Unit Tests

Unit tests are used for fast, relatively simple logic. They can be run with `cargo test`. The dev container is recommended
for executing since it does not require having a proper local setup.

To run locally you will need to first disable mise if you are using it.

### E2E Tests

Like unit tests, the e2e tests should be run either in the dev container (recommended) or with mise disabled locally.

## Dependencies

- [rust](https://www.rust-lang.org/) stable 1.70.0+ (I test with the beta channel locally, but CI uses stable, you can use whatever)
- [just](https://github.com/casey/just) this should be removed in favor of mise tasks but it's still used for some things

## Tasks

Mise uses mise itself to run tasks. See available tasks with `mise tasks`:

```shell
~/src/mise ❯ mise tasks
build                                           ~/src/mise/.mise.toml          
clean                                           ~/src/mise/.mise.toml          
format                                          ~/src/mise/.mise.toml          
lint                                            ~/src/mise/.mise/config.toml   
lint-fix                                        ~/src/mise/.mise.toml          
release                                         ~/src/mise/.mise.toml          
render-all                                      ~/src/mise/.mise.toml          
render-completions                              ~/src/mise/.mise.toml          
render-help                                     ~/src/mise/.mise.toml          
render-mangen                                   ~/src/mise/.mise.toml          
signal-test                                     ~/src/mise/.mise.toml          
snapshots           Update test snapshots       ~/src/mise/.mise.toml          
test                                            ~/src/mise/.mise.toml          
test:e2e                                        ~/src/mise/.mise.toml          
```

## [deprecated] Just

_Note: these tasks are being moved over to `mise run` tasks but not all of them have been migrated yet._

Just should be used for just about every task. Here is a full list of its tasks:

```shell
~/src/mise ❯ just --list
Available recipes:
    build *args           # just `cargo build`
    b *args               # alias for `build`
    default               # defaults to `just test`
    lint                  # clippy, cargo fmt --check, and just --fmt
    l                     # alias for `lint`
    lint-fix              # runs linters but makes fixes when possible
    lf                    # alias for `lint-fix`
    test *args            # run all test types
    t *args               # alias for `test`
    test-coverage         # run unit tests w/ coverage
    test-e2e TEST=("all") # specify a test name to run a single test
    e TEST=("all")        # alias for `test-e2e`
    test-unit *args       # run the rust "unit" tests
```

## Setup

Shouldn't require anything special I'm aware of, but `just build` is a good sanity check to run and make sure it's all working.

## Running the CLI

I put a shim for `cargo run` that makes it easy to run build + run mise in dev mode. It's at `.bin/mise`. What I do is add this to PATH
with direnv. Here is my `.envrc`:

```shell
source_up_if_exists
PATH_add "$(expand_path .bin)"
```

Now I can just run `mise` as if I was using an installed version and it will build it from source every time there are changes.

You don't have to do this, but it makes things like `mise activate` a lot easier to setup.

## Running Tests

- Run only unit tests: `just test-unit`
- Run only E2E tests: `just test-e2e`
- Run all tests: `just test`

## Releasing

Run `just release -x [minor|patch]`. (minor if it is the first release in a month)

## Linting

- Lint codebase: `mise run lint`
- Lint and fix codebase: `mise run lint:fix`

## Generating readme and shell completion files

```shell
mise run render
```

## Testing packaging

This is only necessary to test if actually changing the packaging setup.

### Ubuntu (apt)

This is for arm64, but you can change the arch to amd64 if you want.

```shell
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

```shell
docker run -ti --rm amazonlinux
yum install -y yum-utils
yum-config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
yum install -y mise
mise -v
```

### Fedora (dnf)

```shell
docker run -ti --rm fedora
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
dnf install -y mise
mise -v
```
