FROM alpine:edge@sha256:115729ec5cb049ba6359c3ab005ac742012d92bbaa5b8bc1a878f1e8f62c0cb8

RUN apk add --no-cache sudo build-base alpine-sdk bash direnv glab atools github-cli jq nodejs \
      && apk fix \
      && adduser -D packager \
      && addgroup packager abuild \
      && echo "packager ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers \
      && mkdir -p /__w && chown packager:packager /__w && chmod 777 /__w
#USER packager
#WORKDIR /home/packager

HEALTHCHECK CMD abuild -v
