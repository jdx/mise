# Contributing

Before submitting a PR, unless it's something obvious, consider filing an issue or simply mention what you plan to do in the [Discord](https://discord.gg/UBa7pJUN7Z).
PRs are often either rejected or need to change significantly after submission so make sure before you start working on something it won't be a wasted effort.

## Development Container

The directory `.devcontainer` contains a Dockerfile that can be used to build a container for local development. This is useful if you want to use a [GitHub Codespace](https://docs.github.com/codespaces), VSCode's remote container feature or a standalone container to develop mise. To use it, you'll need to have Docker Desktop installed and running.

Build and run the container with the following commands:

```shell
cd .devcontainer
docker build  -t local/misedevcontainer .
docker run --rm -it -v "$(pwd)"/../:/workspaces/cached local/misedevcontainer
```

To use the container with VSCode, you'll need to install the [Remote - Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) extension. Once installed, you can open the project in a container by opening the Command Palette (F1) and selecting "Remote-Containers: Open Folder in Container...". Select the root of the mise project and the container will be built and started.

## Dependencies

- [rust](https://www.rust-lang.org/) stable 1.66.1+ (it might be compatible with earlier, but I haven't tested that). As of this writing: 1.67.0 but GH actions will use the latest stable whenever it runs.
- [just](https://github.com/casey/just) any version should do, but as of this writing I'm on 1.13.0
- [md-magic](https://github.com/DavidWells/markdown-magic)
- [shfmt](https://github.com/mvdan/sh)
- [shellcheck](https://www.shellcheck.net/)

(you'd think we'd use mise to fetch these but frankly it's kind of a pain to dogfood mise while testing it)

## Just

Just should be used for just about every task. Here is a full list of its
tasks:

```shell
~/src/mise ❯ just --list
Available recipes:
    build *args           # just `cargo build`
    b *args               # alias for `build`
    clean                 # delete built files
    default               # defaults to `just test`
    lint                  # clippy, cargo fmt --check, and just --fmt
    lint-fix              # runs linters but makes fixes when possible
    pre-commit            # called by lefthook precommit hook
    release *args         # create/publish a new version of mise
    render-completions    # regenerate shell completion files
    render-help           # regenerate README.md
    render-mangen         # regenerate manpages
    test *args            # run all test types
    t *args               # alias for `test`
    test-coverage         # run unit tests w/ coverage
    test-e2e TEST=("all") # specify a test name to run a single test
    e TEST=("all")        # alias for `test-e2e`
    test-unit *args       # run the rust "unit" tests
    test-update-snapshots # update all test snapshot files
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

- Lint codebase: `just lint`
- Lint and fix codebase: `just lint-fix`

## Generating readme and shell completion files

```shell
just pre-commit
```

## [optional] Pre-commit hook

This project uses lefthook which will automatically install a pre-commit hook:

```shell
brew install lefthook # or install via some other means
lefthook install
git commit
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
