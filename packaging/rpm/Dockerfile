FROM fedora:43@sha256:2c0d72b2c18bddb61a611cf29a2aae4c7a3ea5e6498eb30d74157afacab6b8fd
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
