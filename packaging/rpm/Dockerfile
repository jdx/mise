FROM fedora:43@sha256:2ad3073554aef41c15b2c52e968ec0540c931bc3dab8c599ca6eb6f32f3f2d14
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
