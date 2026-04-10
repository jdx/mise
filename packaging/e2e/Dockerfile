FROM ubuntu:26.04@sha256:cc925e589b7543b910fea57a240468940003fbfc0515245a495dd0ad8fe7cef1

LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise e2e test environment"
LABEL org.opencontainers.image.licenses=MIT

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH="/opt/node/bin:$PATH"

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN <<EOT
set -euxo pipefail
mkdir -p /opt/node

apt-get update && apt-get install -y --no-install-recommends \
    autoconf \
    bash \
    bison \
    build-essential \
    cargo \
    curl \
    direnv \
    fd-find \
    fish \
    git \
    jq \
    libffi-dev \
    libicu-dev \
    libncurses-dev \
    libreadline-dev \
    libssl-dev \
    libxml2-dev \
    libyaml-dev \
    pipx \
    pkg-config \
    python3-full \
    python3-venv \
    unzip \
    xz-utils \
    zlib1g-dev \
    zsh

ln -sf /usr/bin/fdfind /usr/bin/fd

# Install Node.js to /opt/node (not /usr/bin) so test_backend_missing_deps
# can verify missing-npm behavior with its restricted PATH=/usr/bin:/bin
NODE_VERSION=22.22.1
ARCH=$(dpkg --print-architecture)
curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${ARCH/amd64/x64}.tar.xz" \
    | tar -xJ --strip-components=1 -C /opt/node
/opt/node/bin/npm install -g nushell
ln -sf /opt/node/bin/node /usr/local/bin/node
ln -sf /opt/node/bin/npm /usr/local/bin/npm
ln -sf /opt/node/bin/npx /usr/local/bin/npx
ln -sf /opt/node/bin/nu /usr/local/bin/nu

apt-get clean
rm -rf /var/lib/apt/lists/*
EOT
