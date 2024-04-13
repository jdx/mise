FROM alpine:edge@sha256:e31c3b1cd47718260e1b6163af0a05b3c428dc01fa410baf72ca8b8076e22e72

RUN apk add --no-cache sudo build-base alpine-sdk bash direnv glab atools \
      && apk fix \
      && adduser -D packager \
      && addgroup packager abuild \
      && echo "packager ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers \
      && mkdir -p /__w && chown packager:packager /__w && chmod 777 /__w
#USER packager
#WORKDIR /home/packager

HEALTHCHECK CMD abuild -v
