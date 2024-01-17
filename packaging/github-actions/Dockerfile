FROM ubuntu:22.04
LABEL maintainer="jdx"

ENV PATH="/root/.cargo/bin:${PATH}"
ENV CARGO_HOME="/root/.cargo"
ENV RUSTUP_HOME="/root/.rustup"
ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=Etc/UTC

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN    apt-get update \
    && apt-get install -y --no-install-recommends \
        autoconf \
        bash \
        build-essential \
        ca-certificates \
        curl \
        direnv \
        fd-find \
        fish \
        git \
        gnupg \
        libbz2-dev \
        libdb-dev \
        libffi-dev \
        libgdbm-dev \
        libgdbm6 \
        libgmp-dev \
        liblzma-dev \
        libncurses5-dev \
        libncursesw5-dev \
        libreadline-dev \
        libreadline6-dev \
        libsqlite3-dev \
        libssl-dev \
        libxml2-dev \
        libxmlsec1-dev \
        libyaml-dev \
        patch \
        pkg-config \
        sudo \
        tk-dev \
        uuid-dev \
        xz-utils \
        zlib1g-dev \
        zsh \
    && ln -s /usr/bin/{fdfind,fd} \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_20.x nodistro main" | tee /etc/apt/sources.list.d/nodesource.list \
    && apt-get update && apt-get install -y --no-install-recommends nodejs \
    && node -v \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && rustup install stable && rustup default stable \
    && rustup toolchain install nightly --component llvm-tools-preview \
    && cargo install \
       cargo-msrv \
    && cargo binstall -y \
        cargo-deny \
        cargo-llvm-cov \
        cargo-machete \
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
