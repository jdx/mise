# syntax=docker/dockerfile:1@sha256:ecfaec9ed6d810b56388c508f4121597bfbba70d41a6dfeee4d8cad5f295fc32
# Distroless mise image: the static musl release binary plus CA roots, nothing
# else. Meant as a `COPY --from=` source for any base image; it can also run
# `mise` directly for commands that do not execute installed tools.
#
# Built by .github/workflows/docker.yml from the signed release assets. The
# build context holds the verified binaries at linux-amd64/mise and
# linux-arm64/mise.

# mise reads the system CA store (rustls-native-roots), so a bare scratch
# image could not reach any HTTPS registry. Alpine ships the bundle as part of
# its base image, so this stage copies a file without running anything and
# never needs emulation.
FROM --platform=$BUILDPLATFORM alpine:3.22@sha256:5291449c3df73caf6ed85e649dec1b9e818b39a5d8c871e97afc13e9cd5e8fa8 AS certs

FROM scratch
ARG TARGETARCH
LABEL maintainer="jdx"
LABEL org.opencontainers.image.source=https://github.com/jdx/mise
LABEL org.opencontainers.image.description="mise is a tool for managing your development environment"
LABEL org.opencontainers.image.licenses=MIT

ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --chmod=755 linux-${TARGETARCH}/mise /usr/local/bin/mise

WORKDIR /mise
ENTRYPOINT ["/usr/local/bin/mise"]
CMD ["--help"]
