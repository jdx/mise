FROM alpine:edge

RUN apk add --no-cache sudo build-base alpine-sdk bash direnv glab atools github-cli jq nodejs \
      && apk fix \
      && adduser -D packager \
      && addgroup packager abuild \
      && echo "packager ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers \
      && mkdir -p /__w && chown packager:packager /__w && chmod 777 /__w
#USER packager
#WORKDIR /home/packager

HEALTHCHECK CMD abuild -v
