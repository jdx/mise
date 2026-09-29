FROM ubuntu:26.04@sha256:da6fc2be547864451aa253836dd926da33623312df4a9a243e35dc877c378a78
LABEL maintainer="jdx"

RUN apt-get update \
    && apt-get install -y \
        build-essential \
        ruby \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN gem install fpm
