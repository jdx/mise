FROM mcr.microsoft.com/devcontainers/rust:1-1-bullseye

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
WORKDIR /workspaces/cached

# Use another target directory to avoid conflicts with the host target directory
ENV CARGO_TARGET_DIR=/workspaces/target
RUN mkdir /workspaces/target \
    # Install rust tools
    && rustup component add clippy llvm-tools rustfmt \
    && cargo install cargo-insta cargo-llvm-cov \
    # Install dependencies
    && export DEBIAN_FRONTEND=noninteractive \
    && curl -fsSL https://deb.nodesource.com/nsolid_setup_deb.sh | bash -s -- 20 \
    && apt-get update \
    && apt-get -y install --no-install-recommends \
    # shells, direnv, shellcheck
    bash fish zsh direnv nodejs shellcheck \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    # shfmt
    && curl -sS https://webi.sh/shfmt | sh \
    # just
    && curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/bin \
    # npm packages
    && npm install -g prettier@3.1.0 markdownlint-cli@0.37.0 markdown-magic@2.6.1

ENTRYPOINT [ "/bin/bash" ]
