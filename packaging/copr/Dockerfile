FROM fedora:45@sha256:8b838b3253cb855b1e5c3e1366fd0cda28cd1cc4c3517d625847172fd4ac6869
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
