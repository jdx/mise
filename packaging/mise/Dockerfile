# syntax=docker/dockerfile:1@sha256:dbbd5e059e8a07ff7ea6233b213b36aa516b4c53c645f1817a4dd18b83cbea56
FROM rust@sha256:da23629cc6826f0c395c8697994cdce0b5ac6850a695c448a101c3cc9cd6a59b as builder
LABEL maintainer="jdx"

WORKDIR /usr/src/mise
COPY . /usr/src/mise/
RUN cargo build --release

FROM rust@sha256:da23629cc6826f0c395c8697994cdce0b5ac6850a695c448a101c3cc9cd6a59b as runtime

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"

COPY --from=builder /usr/src/mise/target/release/mise /usr/local/bin/mise

RUN <<EOT
set -euxo pipefail

apt-get update && apt-get install -y \
    jq                               \
    python3-full                     \
    python3-pip
rm -rf /var/lib/apt/lists/* && apt-get clean

mise use -g python@latest

mise -v
EOT

WORKDIR /mise
ENTRYPOINT ["mise"]
CMD ["--help"]
