FROM ubuntu:26.04@sha256:2260313b31c8c011cd2eebe728008efac1b3982be73eb71348ea2648d2c0e09b
LABEL maintainer="jdx"

RUN apt-get update \
    && apt-get install -y \
        build-essential \
        ruby \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN gem install fpm
