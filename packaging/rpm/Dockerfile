FROM fedora:43@sha256:6cd815d862109208adf6040ea13391fe6aeb87a9dc80735c2ab07083fdf5e03a
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
