FROM mcr.microsoft.com/devcontainers/rust:0-1-bullseye

WORKDIR /workspaces/cached

# Use another target directory to avoid conflicts with the host target directory
RUN mkdir /workspaces/target
ENV CARGO_TARGET_DIR=/workspaces/target

# Install rust tools
RUN rustup component add clippy llvm-tools rustfmt
RUN cargo install cargo-insta cargo-llvm-cov

# Install dependencies
RUN export DEBIAN_FRONTEND=noninteractive \
    && curl -fsSL https://deb.nodesource.com/setup_18.x | bash - \
    && apt-get update \
    && apt-get -y install --no-install-recommends \
    # shells, direnv, shellcheck
    bash fish zsh direnv nodejs shellcheck \
    && apt-get clean \
    # shfmt
    && curl -sS https://webi.sh/shfmt | sh \
    # just
    && curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to /usr/bin

ENTRYPOINT [ "/bin/bash" ]
