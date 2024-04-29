FROM mcr.microsoft.com/devcontainers/rust:1
LABEL maintainer="jdx"

ENV GOPROXY=direct
ENV MISE_EXPERIMENTAL=1
ENV MISE_TRUSTED_CONFIG_PATHS="/mise"
ENV dev="/mise/.dev"
ENV MISE_DATA_DIR="$dev/mise"
ENV MISE_STATE_DIR="$dev/mise"
ENV MISE_CONFIG_DIR="$dev/mise"
ENV MISE_CACHE_DIR="$dev/mise/cache"
ENV CARGO_TARGET_DIR="/tmp/mise/target"

# TODO: deno does not currently support linux/arm64 making it hard to use this on macs
ENV MISE_DISABLE_TOOLS="deno"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN    apt-get update \
    && apt-get install -y --no-install-recommends \
    direnv \
    fd-find \
    fish \
    python3-venv \
    && ln -s /usr/bin/{fdfind,fd} \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_20.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list \
    && apt-get update && apt-get install -y --no-install-recommends nodejs \
    && node -v \
    && curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && rustup toolchain install nightly --component llvm-tools-preview \
    && cargo binstall -y \
    cargo-deny \
    cargo-insta \
    cargo-llvm-cov \
    cargo-machete \
    # cargo-msrv \ this is very slow to compile
    cargo-nextest \
    cross \
    zipsign \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && rustc -vV \
    && cargo -V \
    && node -v \
    && npm -v \
    && python3 -v \
    && rm -rf "$CARGO_HOME/registry" && ln -sv /tmp/mise/registry "$CARGO_HOME/registry" && mkdir -p /tmp/mise/registry
