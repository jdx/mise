# syntax=docker/dockerfile:1
FROM rust as builder
LABEL maintainer="jdx"

WORKDIR /usr/src/rtx
COPY . /usr/src/rtx/
RUN cargo build --release

FROM rust as runtime

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV RTX_DATA_DIR="/rtx"
ENV RTX_CONFIG_DIR="/rtx"
ENV RTX_CACHE_DIR="/rtx/cache"
ENV PATH="/rtx/shims:$PATH"

COPY --from=builder /usr/src/rtx/target/release/rtx /usr/local/bin/rtx

RUN <<EOT
set -euxo pipefail

apt-get update && apt-get install -y \
    jq                               \
    python3-full                     \
    python3-pip
rm -rf /var/lib/apt/lists/* && apt-get clean

ln -s ./python3 /usr/bin/python

rtx -v
EOT

WORKDIR /rtx
ENTRYPOINT ["rtx"]
CMD ["--help"]
