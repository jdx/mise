# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
# Runnable mise image on Debian slim: the glibc release binary plus the
# packages most tool installs need (CA roots, curl, git). Nothing is
# preinstalled through mise; `mise install` does that for the project.
#
# Built by .github/workflows/docker.yml from the signed release assets. The
# build context holds the verified binaries at linux-amd64/mise and
# linux-arm64/mise.
FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a
ARG TARGETARCH
LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise is a tool for managing your development environment"
LABEL org.opencontainers.image.licenses=MIT

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/*

ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"

COPY --chmod=755 linux-${TARGETARCH}/mise /usr/local/bin/mise
RUN mise --version

WORKDIR /mise
# No ENTRYPOINT: CI runners such as GitLab start containers with their own
# shell command, and `FROM` users expect a plain base image.
CMD ["mise", "--help"]
