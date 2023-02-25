FROM ubuntu:22.04
LABEL maintainer="jdxcode"

RUN apt-get update \
    && apt-get install -y \
        build-essential \
        ruby \
    && apt-get clean

RUN gem install fpm
