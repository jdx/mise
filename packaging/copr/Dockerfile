FROM fedora:43@sha256:6cd815d862109208adf6040ea13391fe6aeb87a9dc80735c2ab07083fdf5e03a
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
