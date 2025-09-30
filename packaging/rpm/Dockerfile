FROM fedora:43@sha256:efbc076ea9463ae19a8babf9bdfe4c8fbb6b163a9db3ff58c25f2be7dbc743a6
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
