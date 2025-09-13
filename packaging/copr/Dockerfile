FROM fedora:43@sha256:9c19fbb641ff1edf956399d41a529e22ae8d6fb41f63c66967ba466acbb70f0e
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
        tar \
        gzip && \
    dnf clean all

# Install copr-cli
RUN pip3 install copr-cli

# Add mise to PATH
ENV PATH="/root/.local/bin:${PATH}"

# Install mise and use it to install cargo-vendor
RUN curl https://mise.run | sh
RUN mise use -g cargo-binstall cargo:cargo-vendor

# Set up RPM build environment
RUN rpmdev-setuptree
