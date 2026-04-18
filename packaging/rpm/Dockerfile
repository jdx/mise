FROM fedora:45@sha256:af8cdc432037f5e8e288bbc26c2b55b96000a911ec5b37959cd464d830f6cc5b
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
