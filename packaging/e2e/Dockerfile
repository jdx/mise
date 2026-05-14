FROM ubuntu:resolute-20260413@sha256:5e275723f82c67e387ba9e3c24baa0abdcb268917f276a0561c97bef9450d0b4

LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise e2e test environment"
LABEL org.opencontainers.image.licenses=MIT

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH="/opt/node/bin:$PATH"
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8

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
    default-jdk-headless \
    direnv \
    fd-find \
    fish \
    git \
    jq \
    libasound2t64 \
    libffi-dev \
    libgl1 \
    libglib2.0-0 \
    libicu-dev \
    libncurses-dev \
    libreadline-dev \
    libssl-dev \
    libsm6 \
    libwayland-client0 \
    libxi6 \
    libxfixes3 \
    libxkbcommon0 \
    libxml2-dev \
    libyaml-dev \
    pipx \
    pkg-config \
    openssh-client \
    python-is-python3 \
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
