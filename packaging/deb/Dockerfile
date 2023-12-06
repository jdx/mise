FROM ubuntu:22.04
LABEL maintainer="jdx"

RUN apt-get update \
    && apt-get install --no-install-recommends -y \
        build-essential \
        ruby \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN gem install fpm
