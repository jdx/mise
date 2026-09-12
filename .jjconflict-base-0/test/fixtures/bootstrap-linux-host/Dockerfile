FROM ubuntu@sha256:786a8b558f7be160c6c8c4a54f9a57274f3b4fb1491cf65146521ae77ff1dc54

ENV container=docker
ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8
ENV LC_ALL=C.UTF-8

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        docker-compose-v2 \
        docker.io \
        nftables \
        openssh-server \
        sudo \
        systemd \
        systemd-sysv \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && systemctl enable docker.service ssh.service

COPY daemon.json /etc/docker/daemon.json

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
