FROM ubuntu:26.04@sha256:a072b64036a738e55bff8f9a9682cbb893bf20c213772effc1de8dee8df1cea9

LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise e2e test environment"
LABEL org.opencontainers.image.licenses=MIT

ENV DEBIAN_FRONTEND=noninteractive

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN <<EOT
set -euxo pipefail

apt-get update && apt-get install -y --no-install-recommends \
    bash \
    bison \
    build-essential \
    curl \
    direnv \
    fd-find \
    fish \
    git \
    jq \
    npm \
    pipx \
    python3-full \
    python3-venv \
    zsh

ln -sf /usr/bin/fdfind /usr/bin/fd
npm install -g nushell

apt-get clean
rm -rf /var/lib/apt/lists/*
EOT
