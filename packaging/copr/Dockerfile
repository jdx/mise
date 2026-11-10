FROM fedora:45@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c
LABEL maintainer="jdx"

# Install packaging dependencies
RUN dnf update -y && \
    dnf install -y \
        rpm-build \
        rpmdevtools \
        python3-pip \
        git \
        rust \
        cargo \
        gcc \
        openssl-devel \
        tar \
        gzip && \
    dnf clean all

# Install copr-cli via dnf to ensure dependencies are properly managed and to avoid potential issues with manual installation
RUN dnf install -y copr-cli && \
    dnf clean all && \
    test "$(readlink -f "$(command -v copr-cli)")" = "/usr/bin/copr-cli" && \
    copr-cli --help >/dev/null

# Set up RPM build environment
RUN rpmdev-setuptree
