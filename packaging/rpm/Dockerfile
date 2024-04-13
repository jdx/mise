FROM fedora:38@sha256:6be1570e3add71ca2af2b47d99e77357d9ac96f6e0ed5e5c30c5dbac6df25a0f
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
