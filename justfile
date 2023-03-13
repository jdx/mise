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
    cargo build --all-features {{ args }}

build-core-plugins:
    #!/usr/bin/env bash
    set -euo pipefail
    for plugin in $(ls core_plugins); do
        if [[ "$plugin" == *.tar.xz ]]; then
            continue
        fi
        echo "Building core plugin: $plugin"
        tar -cJf core_plugins/$plugin.tar.xz -C core_plugins $plugin
    done

alias t := test

# run all test types
test *args: (test-unit args) test-e2e lint

# update all test snapshot files
test-update-snapshots:
    find . -name '*.snap' -delete
    cargo insta test --accept --features clap_mangen

# run the rust "unit" tests
test-unit *args:
    cargo test --features clap_mangen {{ args }}

# runs the E2E tests in ./e2e
test-e2e: build
    ./e2e/run_all_tests

# run unit tests w/ coverage
test-coverage:
    #!/usr/bin/env bash
    set -euxo pipefail
    source <(cargo llvm-cov show-env --export-prefix) 
    cargo llvm-cov clean --workspace

    if [[ -n "${RTX_GITHUB_BOT_TOKEN:-}" ]]; then
    	export GITHUB_API_TOKEN="$RTX_GITHUB_BOT_TOKEN"
    fi

    cargo test --features clap_mangen
    cargo build --all-features
    PATH="$PWD/target/debug:$PATH" ./e2e/run_all_tests
    RTX_SELF_UPDATE_VERSION=1.0.0 ./target/debug/rtx self-update <<EOF
    y
    EOF
    cargo build
    rtx implode
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
    shellcheck scripts/*.sh
    shfmt -d scripts/*.sh
    just --unstable --fmt --check

# runs linters but makes fixes when possible
lint-fix:
    cargo clippy --all-features --fix --allow-staged --allow-dirty
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
    NO_COLOR=1 rtx complete -s bash > completions/rtx.bash
    NO_COLOR=1 rtx complete -s zsh > completions/_rtx
    NO_COLOR=1 rtx complete -s fish > completions/rtx.fish

# regenerate manpages
render-mangen: build
    NO_COLOR=1 rtx mangen

# called by husky precommit hook
pre-commit: render-help render-completions render-mangen
    git add README.md
    git add completions
    git add man

release *args:
    cargo release {{ args }}
