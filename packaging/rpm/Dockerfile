FROM fedora:45@sha256:aacbc26b38361ba0cb3d10f268da40b61b36aed6ed6fe5831a35871636bedb06
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
