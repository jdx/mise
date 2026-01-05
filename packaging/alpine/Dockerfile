FROM alpine:edge@sha256:ea71a031ed91cd46b00d438876550bc765da43b4ae40f331a12daf62f0937758

RUN apk add --no-cache sudo build-base alpine-sdk bash direnv glab atools github-cli jq nodejs \
      && apk fix \
      && adduser -D packager \
      && addgroup packager abuild \
      && echo "packager ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers \
      && mkdir -p /__w && chown packager:packager /__w && chmod 777 /__w
#USER packager
#WORKDIR /home/packager

HEALTHCHECK CMD abuild -v
