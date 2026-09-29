# The machines the showreel's terminal captures are recorded on.
#
#   machine1  the person's own machine (C1 to C17)
#   machine2  a fresh machine with mise installed (C18, C19): machine1 plus
#             systemd, so `mise bootstrap` can start the history watcher as
#             a user service. It boots /sbin/init when run privileged; run
#             with any other command it is the same machine without a
#             service manager (the fallback, which has no watcher service).
#
# The images hold only the operating system and the user. The mise binary,
# the recorder and the fixtures are copied into each container at run time
# by `mise run docs:showreel-capture`, so changing them never rebuilds these
# images, and a new mise release never changes them either.
#
# Everything is pinned, so two builds months apart install the same bytes:
# the base image by digest, and the Debian packages by the archive snapshot
# that image was built from (its /etc/apt/sources.list.d/debian.sources
# names it in a comment). The recorder writes the package versions it ran
# with into out/runs/<run>/machine*.json.
FROM debian:trixie-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a AS machine1

# apt reads the snapshot over http; the Release files are signed and checked
# against the image's own keyring. A snapshot's Release files expire, so
# their Valid-Until is not checked. Every fetch is retried.
RUN sed -i \
      -e 's|^URIs: http://deb.debian.org/debian-security$|URIs: http://snapshot.debian.org/archive/debian-security/20260918T000000Z|' \
      -e 's|^URIs: http://deb.debian.org/debian$|URIs: http://snapshot.debian.org/archive/debian/20260918T000000Z|' \
      /etc/apt/sources.list.d/debian.sources \
 && test "$(grep -c '^URIs: http://snapshot.debian.org/archive/debian\(-security\)\?/20260918T000000Z$' /etc/apt/sources.list.d/debian.sources)" = 2 \
 && printf '%s\n' 'Acquire::Retries "8";' 'Acquire::http::Timeout "60";' \
      'Acquire::Check-Valid-Until "false";' >/etc/apt/apt.conf.d/80showreel

# libatomic1: Node 26's linux-x64 build links against it.
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      git \
      libatomic1 \
      python3 \
      python3-pyte \
      tmux \
      zsh \
 && rm -rf /var/lib/apt/lists/*

# The person in the reel. HOME=/home/you, so every path mise prints is
# ~/work/... or /home/you/..., never a build machine's path.
RUN useradd --create-home --uid 1000 --shell /usr/bin/zsh you \
 && mkdir /rig /rig-bin /out

ENV LANG=C.UTF-8

FROM machine1 AS machine2

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      dbus \
      dbus-user-session \
      libpam-systemd \
      systemd \
      systemd-sysv \
 && rm -rf /var/lib/apt/lists/*

# A fresh machine: no ~/.zshrc (bootstrap writes it), the prompt comes from
# /etc/zsh/zshrc, and zsh's new-user wizard is removed so an empty home
# starts straight at the prompt.
RUN rm /usr/share/zsh/*/scripts/newuser \
 && printf "PROMPT='%%~ \$ '\n" >> /etc/zsh/zshrc

# The user manager starts at boot (what `loginctl enable-linger you` does),
# so `systemctl --user` works for a shell started with `docker exec`. Units
# that cannot work in a container are masked so boot finishes cleanly.
RUN mkdir -p /var/lib/systemd/linger \
 && touch /var/lib/systemd/linger/you \
 && systemctl mask getty@tty1.service systemd-modules-load.service \
      console-getty.service systemd-firstboot.service

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
