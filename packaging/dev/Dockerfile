FROM mcr.microsoft.com/devcontainers/rust:1@sha256:a055200cfc0e0ce9a66018a00887dca391eefdb6c3d87d8178f7aa7703ff2c71
LABEL maintainer="jdx"

ENV GOPROXY=direct
ENV MISE_EXPERIMENTAL=1
ENV MISE_TRUSTED_CONFIG_PATHS="/mise"
ENV dev="/mise/.dev"
ENV MISE_DATA_DIR="$dev/mise"
ENV MISE_STATE_DIR="$dev/mise"
ENV MISE_CONFIG_DIR="$dev/mise"
ENV MISE_CACHE_DIR="$dev/mise/cache"

# TODO: deno does not currently support linux/arm64 making it hard to use this on macs
ENV MISE_DISABLE_TOOLS="deno"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN    apt-get update \
    && apt-get install -y --no-install-recommends \
       direnv \
       fd-find \
       fish \
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
        just \
        zipsign \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && rustc -vV \
    && cargo -V \
    && node -v \
    && npm -v \
    && just --version
#    && mkdir -p $dev/cargo \
#    && mv "$CARGO_HOME/registry" $dev/cargo/registry && ln -s $dev/cargo/registry "$CARGO_HOME/registry" \
#    && mkdir $dev/cargo/git && ln -s $dev/cargo/git "$CARGO_HOME/git"
