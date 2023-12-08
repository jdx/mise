FROM alpine:edge
LABEL maintainer="jdx"

ENV RTX_DATA_DIR="/rtx"
ENV RTX_CONFIG_DIR="/rtx"
ENV RTX_CACHE_DIR="/rtx/cache"
ENV PATH="/rtx/shims:$PATH"

RUN apk add --no-cache \
    curl \
    bash \
    git \
    rtx \
    wget \
    && rtx --version

WORKDIR /rtx
ENTRYPOINT ["rtx"]
CMD ["--help"]
