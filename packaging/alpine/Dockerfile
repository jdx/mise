FROM alpine:3
RUN apk add --no-cache sudo build-base alpine-sdk bash direnv glab atools
RUN apk fix
RUN adduser -D packager
RUN addgroup packager abuild
RUN echo "packager ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers
RUN mkdir -p /__w && chown packager:packager /__w && chmod 777 /__w
#USER packager
#WORKDIR /home/packager
