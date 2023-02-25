FROM fedora:38
LABEL maintainer="jdxcode"

RUN dnf install -y rpm-build rpm-sign ruby ruby-devel gcc
RUN gem install fpm
RUN dnf install -y createrepo
