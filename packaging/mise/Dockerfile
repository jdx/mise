# syntax=docker/dockerfile:1@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769
FROM rust@sha256:5b1e3484ddcd22a3738c0ec34a5e98bf19382eb295fb6db54295e62379119040 AS builder
LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise is a tool for managing your development environment"
LABEL org.opencontainers.image.licenses=MIT

WORKDIR /usr/src/mise
COPY . /usr/src/mise/
RUN cargo build --release

FROM rust@sha256:5b1e3484ddcd22a3738c0ec34a5e98bf19382eb295fb6db54295e62379119040 AS runtime

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
