# Build environment for Anthropic's OSS Scanner (https://red.anthropic.com/oss-scanner/).
# The scanner builds this with the repository as the build context and runs the audit offline,
# so everything the audit needs must be fetched here.
FROM rust@sha256:1bcff4befb740599103a2c7cb51058e14479b2e35e3a34a3f0dc4ede09927488
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      cmake clang libclang-dev pkg-config libssl-dev git jq zsh fish python3 \
    && rm -rf /var/lib/apt/lists/*

# scanner contract: the checkout lives inside the image, at /src
COPY . /src
WORKDIR /src

# Debug build keeps debug info and assertions; the binary is target/debug/mise.
# Fetch and compile every dependency now, because the audit has no network.
RUN cargo build --ignore-rust-version --all-features

ENV PATH="/src/target/debug:$PATH"
RUN mise --version
