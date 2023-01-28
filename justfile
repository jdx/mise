set shell := ["bash", "-uc"]

export RTX_DATA_DIR := "/tmp/rtx"
export PATH := env_var("PWD") + "/target/debug:" + env_var("PATH")
export RTX_MISSING_RUNTIME_BEHAVIOR := "autoinstall"
export RUST_TEST_THREADS := "1"

default: test

alias b := test

build *args:
    cargo build {{ args }}

alias t := test

test: test-unit test-e2e

test-setup: build

test-update-snapshots: test-setup
    cargo insta test --accept

test-unit: test-setup
    cargo test

test-e2e: test-setup build
    ./e2e/run_all_tests

test-coverage: clean test-setup
    cargo +nightly tarpaulin \
      --all-features --workspace \
      --timeout 120 --out Xml --ignore-tests

clean:
    cargo clean
    rm -rf target
    rm -rf *.profraw
    rm -rf coverage

lint:
    cargo clippy
    cargo fmt --all -- --check
    just --unstable --fmt --check

lint-fix:
    cargo clippy --fix --allow-staged --allow-dirty
    cargo fmt --all
    just --unstable --fmt

render-help:
    ./.bin/rtx render-help > README.md

render-completions:
    ./.bin/rtx complete -s bash > completions/rtx.bash
    ./.bin/rtx complete -s zsh > completions/_rtx
    ./.bin/rtx complete -s fish > completions/rtx.fish
