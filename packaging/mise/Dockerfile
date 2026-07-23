# syntax=docker/dockerfile:1@sha256:87999aa3d42bdc6bea60565083ee17e86d1f3339802f543c0d03998580f9cb89
FROM rust@sha256:9a2cd304a852f05d3352f75bc2775242371c0169a72dbb40d5d881379d571989 AS builder
LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise is a tool for managing your development environment"
LABEL org.opencontainers.image.licenses=MIT

WORKDIR /usr/src/mise
COPY . /usr/src/mise/
RUN apt-get update && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
RUN cargo build --release

FROM rust@sha256:9a2cd304a852f05d3352f75bc2775242371c0169a72dbb40d5d881379d571989 AS runtime

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"
ENV MISE_CACHE_PRUNE_AGE="10y"

COPY --from=builder /usr/src/mise/target/release/mise /usr/local/bin/mise

RUN <<EOT
set -euxo pipefail

apt-get update && apt-get install -y \
    jq                               \
    python3-full                     \
    python3-pip
rm -rf /var/lib/apt/lists/* && apt-get clean

mise use -g node@lts python@latest

mise -v
EOT

WORKDIR /mise
ENTRYPOINT ["mise"]
CMD ["--help"]
