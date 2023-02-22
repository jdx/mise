set shell := ["bash", "-uc"]

export RTX_DATA_DIR := "/tmp/rtx"
export PATH := env_var("PWD") + "/target/debug:" + env_var("PATH")
export RTX_MISSING_RUNTIME_BEHAVIOR := "autoinstall"
export RUST_TEST_THREADS := "1"

# defaults to `just test`
default: test

alias b := test

# just `cargo build`
build *args:
    cargo build {{ args }}

alias t := test

# run all test types
test: test-unit test-e2e

# prepare repo to execute tests
test-setup: build

# update all test snapshot files
test-update-snapshots: test-setup
    find . -name '*.snap' -delete
    cargo insta test --accept

# run the rust "unit" tests
test-unit: test-setup
    cargo test

# runs the E2E tests in ./e2e
test-e2e: test-setup build
    ./e2e/run_all_tests

# run unit tests w/ coverage
test-coverage:
    #!/usr/bin/env bash
    set -euxo pipefail
    source <(cargo llvm-cov show-env --export-prefix) 
    cargo llvm-cov clean --workspace 

    cargo test
    cargo build
    PATH="$PWD/target/debug:$PATH" ./e2e/run_all_tests
    cargo llvm-cov report --html
    cargo llvm-cov report --lcov --output-path lcov.info

# delete built files
clean:
    cargo clean
    rm -rf target
    rm -rf *.profraw
    rm -rf coverage

# clippy, cargo fmt --check, and just --fmt
lint:
    cargo clippy
    cargo fmt --all -- --check
    just --unstable --fmt --check

# runs linters but makes fixes when possible
lint-fix:
    cargo clippy --fix --allow-staged --allow-dirty
    cargo fmt --all
    just --unstable --fmt

# regenerate README.md
render-help:
    ./.bin/rtx render-help > README.md
    ./scripts/gh-md-toc --insert --no-backup --hide-footer --skip-header README.md

# regenerate shell completion files
render-completions:
    ./.bin/rtx complete -s bash > completions/rtx.bash
    ./.bin/rtx complete -s zsh > completions/_rtx
    ./.bin/rtx complete -s fish > completions/rtx.fish

# called by husky precommit hook
pre-commit: lint render-help render-completions
    git add README.md
    git add completions
