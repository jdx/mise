FROM fedora:45@sha256:8b838b3253cb855b1e5c3e1366fd0cda28cd1cc4c3517d625847172fd4ac6869
LABEL maintainer="jdx"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc \
      && gem install fpm \
      && dnf install -y createrepo \
      && dnf clean all
