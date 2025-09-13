FROM fedora:43@sha256:9c19fbb641ff1edf956399d41a529e22ae8d6fb41f63c66967ba466acbb70f0e
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
