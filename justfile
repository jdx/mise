set shell := ["bash", "-uc"]

export RTX_DATA_DIR := "/tmp/rtx"
export PATH := env_var_or_default("CARGO_TARGET_DIR", "$PWD/target") + "/debug:" + env_var("PATH")
export RTX_MISSING_RUNTIME_BEHAVIOR := "autoinstall"
export RUST_TEST_THREADS := "1"

# defaults to `just test`
default: test

alias b := build
alias e := test-e2e
alias t := test

# just `cargo build`
build *args:
    cargo build {{ args }}

# run all test types
test *args: (test-unit args) test-e2e lint

# update all test snapshot files
test-update-snapshots:
    cargo insta test --accept

# run the rust "unit" tests
test-unit *args:
    cargo test {{ args }}

# runs the E2E tests in ./e2e

# specify a test name to run a single test
test-e2e TEST=("all"):
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{ TEST }}" = all ]; then
        ./e2e/run_all_tests
    else
        FILES="$(fd {{ TEST }} e2e/)"
        ./e2e/run_test "$FILES"
    fi

# run unit tests w/ coverage
test-coverage:
    #!/usr/bin/env bash
    set -euxo pipefail
    source <(cargo llvm-cov show-env --export-prefix)
    cargo llvm-cov clean --workspace

    if [[ -n "${RTX_GITHUB_BOT_TOKEN:-}" ]]; then
    	export GITHUB_API_TOKEN="$RTX_GITHUB_BOT_TOKEN"
    fi

    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
    export PATH="${CARGO_TARGET_DIR}/debug:$PATH"
    cargo test
    cargo build --all-features
    ./e2e/run_all_tests
    rtx trust
    RTX_SELF_UPDATE_VERSION=1.0.0 rtx self-update <<EOF
    y
    EOF
    cargo build
    rtx implode
    cargo llvm-cov report --html
    cargo llvm-cov report --lcov --output-path lcov.info
    cargo llvm-cov report

# delete built files
clean:
    cargo clean
    rm -f lcov.info
    rm -rf e2e/.{asdf,config,local,rtx}/
    rm -rf target
    rm -rf *.profraw
    rm -rf coverage

# clippy, cargo fmt --check, and just --fmt
lint:
    cargo clippy
    cargo fmt --all -- --check
    shellcheck scripts/*.sh
    shfmt -d scripts/*.sh
    just --unstable --fmt --check

# runs linters but makes fixes when possible
lint-fix:
    cargo clippy --fix --allow-staged --allow-dirty
    cargo fmt --all
    shellcheck scripts/*.sh
    shfmt -w scripts/*.sh
    just --unstable --fmt

# regenerate README.md
render-help: build
    NO_COLOR=1 rtx render-help
    npx markdown-magic

# regenerate shell completion files
render-completions: build
    NO_COLOR=1 rtx completion bash > completions/rtx.bash
    NO_COLOR=1 rtx completion zsh > completions/_rtx
    NO_COLOR=1 rtx completion fish > completions/rtx.fish

# regenerate manpages
render-mangen:
    NO_COLOR=1 cargo xtask mangen

# called by lefthook precommit hook
pre-commit: render-help render-completions render-mangen
    git add README.md
    git add completions
    git add man

# create/publish a new version of rtx
release *args:
    cargo release {{ args }}
