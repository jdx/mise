FROM rust as builder
LABEL maintainer="jdx"
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ENV RTX_DATA_DIR="/rtx"
ENV RTX_CONFIG_DIR="/rtx"
ENV RTX_CACHE_DIR="/rtx/cache"
ENV PATH="/rtx/shims:$PATH"
WORKDIR /usr/src/rtx

COPY . /usr/src/rtx/

RUN cargo build --release

FROM rust as runtime

COPY --from=builder /usr/src/rtx/target/release/rtx /usr/local/bin/rtx

WORKDIR /rtx
ENTRYPOINT ["rtx"]
CMD ["--help"]
