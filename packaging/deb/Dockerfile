FROM ubuntu:22.04@sha256:77906da86b60585ce12215807090eb327e7386c8fafb5402369e421f44eff17e
LABEL maintainer="jdx"

RUN apt-get update \
    && apt-get install -y \
        build-essential \
        ruby \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN gem install fpm
