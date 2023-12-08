FROM ubuntu:22.04
LABEL maintainer="jdx"
SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ENV RTX_DATA_DIR="/rtx"
ENV RTX_CONFIG_DIR="/rtx"
ENV RTX_CACHE_DIR="/rtx/cache"
ENV PATH="/rtx/shims:/rtx/bin:$PATH"

ENV RTX_INSTALL_PATH="/rtx/bin/rtx"

RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    bash \
    git \
    wget \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    # TODO: build from this source
    && curl -fsSL https://rtx.pub/install.sh | sh \
    && rtx --version

WORKDIR /rtx
ENTRYPOINT ["rtx"]
CMD ["--help"]
