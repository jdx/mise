FROM fedora:45@sha256:0c1f63ed8fb818fad16cf6ae091598c410a21d2e1a9adf183beb93189299bfba
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
