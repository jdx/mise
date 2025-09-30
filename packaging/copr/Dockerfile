FROM fedora:43@sha256:efbc076ea9463ae19a8babf9bdfe4c8fbb6b163a9db3ff58c25f2be7dbc743a6
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
