# Contributing

## Development Container

The directory `.devcontainer` contains a Dockerfile that can be used to build a container for local development. This is useful if you want to use either VSCode's remote container feature or a standalone container to develop rtx. To use it, you'll need to have Docker Desktop installed and running.

Build and run the container with the following commands:

```shell
cd .devcontainer
docker build  -t local/rtxdevcontainer .
docker run --rm -it -v "$(pwd)"/../:/workspaces/cached local/rtxdevcontainer
```

To use the container with VSCode, you'll need to install the [Remote - Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) extension. Once installed, you can open the project in a container by opening the Command Palette (F1) and selecting "Remote-Containers: Open Folder in Container...". Select the root of the rtx project and the container will be built and started.

## Dependencies

* [rust](https://www.rust-lang.org/) stable 1.66.1+ (it might be compatible with earlier, but I haven't tested that). As of this writing: 1.67.0 but GH actions will use the latest stable whenever it runs.
* [just](https://github.com/casey/just) any version should do, but as of this writing I'm on 1.13.0

(you'd think we'd use rtx to fetch these but frankly it's kind of a pain to dogfood rtx while testing it)

## Just

Just should be used for just about every task. Here is a full list of its
tasks:

```
~/src/rtx ❯ just --list
Available recipes:
    build *args           # just `cargo build`
    clean                 # delete built files
    default               # defaults to `just test`
    lint                  # clippy, cargo fmt --check, and just --fmt
    lint-fix              # runs linters but makes fixes when possible
    pre-commit            # called by husky precommit hook
    release *args
    render-completions    # regenerate shell completion files
    render-help           # regenerate README.md
    render-mangen         # regenerate manpages
    test *args            # run all test types
    b *args               # alias for `test`
    t *args               # alias for `test`
    test-coverage         # run unit tests w/ coverage
    test-e2e              # runs the E2E tests in ./e2e
    test-unit *args       # run the rust "unit" tests
    test-update-snapshots # update all test snapshot files
```

## Setup

Shouldn't require anything special I'm aware of, but `just build` is a good sanity check to run and make sure it's all working.

## Running the CLI

I put a shim for `cargo run` that makes it easy to run build + run rtx in dev mode. It's at `.bin/rtx`. What I do is add this to PATH
with direnv. Here is my `.envrc`:

```
source_up_if_exists
PATH_add "$(expand_path .bin)"
```

Now I can just run `rtx` as if I was using an installed version and it will build it from source every time there are changes.

You don't have to do this, but it makes things like `rtx activate` a lot easier to setup.

## Running Tests

* Run only unit tests: `just test-unit`
* Run only E2E tests: `just test-e2e`
* Run all tests: `just test`

## Linting

* Lint codebase: `just lint`
* Lint and fix codebase: `just lint-fix`

## Generating readme and shell completion files

```
just pre-commit
```

## [optional] Pre-commit hook

This project uses lefthook which will automatically install a pre-commit hook:

```
brew install lefthook # or install via some other means
lefthook install
git commit
```

## Testing packaging

I test these with finch, but docker should work the same. This is only necessary to test
if actually changing the packaging setup.

### Ubuntu (apt)

This is for arm64, but you can change the arch to amd64 if you want.

```
finch run -ti --rm ubuntu
apt update -y
apt install gpg sudo wget curl
wget -qO - https://rtx.pub/gpg-key.pub | gpg --dearmor | sudo tee /usr/share/keyrings/rtx-archive-keyring.gpg 1> /dev/null
echo "deb [signed-by=/usr/share/keyrings/rtx-archive-keyring.gpg arch=arm64] https://rtx.pub/deb stable main" | sudo tee /etc/apt/sources.list.d/rtx.list
apt update
apt install -y rtx
rtx -V
```

### Amazon Linux 2 (yum)

```
finch run -ti --rm amazonlinux
yum install -y yum-utils
yum-config-manager --add-repo https://rtx.pub/rpm/rtx.repo
yum install -y rtx
rtx -v
```

### Fedora (dnf)

```
finch run -ti --rm fedora
dnf install -y dnf-plugins-core
dnf config-manager --add-repo https://rtx.pub/rpm/rtx.repo
dnf install -y rtx
rtx -v
```
