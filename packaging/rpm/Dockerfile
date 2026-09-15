FROM fedora:46@sha256:a9bab18d01cf2c2cf62f3e79c72623405bce14ac062995cc6651a3073c802e41
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
