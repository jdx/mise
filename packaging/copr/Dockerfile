FROM fedora:45@sha256:0c1f63ed8fb818fad16cf6ae091598c410a21d2e1a9adf183beb93189299bfba
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
