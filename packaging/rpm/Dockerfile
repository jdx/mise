FROM fedora:43
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
