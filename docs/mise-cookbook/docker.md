---
description: "Use the official mise images or install a pinned, verified mise inside your own image, use it to run project commands, or preinstall tools outside user home directories for shared development containers."
---

# Docker Cookbook

Use the official mise images, copy a pinned mise binary into your own image,
or install mise from a package or verified release download. Then use mise to
run project commands, or preinstall tools outside user home directories for
shared development containers. Building these examples requires Docker and a
running container engine.

## Official images

Every release publishes two images to `ghcr.io/jdx/mise` and Docker Hub
(`jdxcode/mise`) for `linux/amd64` and `linux/arm64`. The mise binary inside
each image is the release asset itself, checked against the release's
minisign-signed checksums before the image is built.

| Tags                                          | Contents                                                                                    | Use it for                                                     |
| --------------------------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `latest`, `2026.9`, `2026.9.11`               | `FROM scratch`: the static musl `mise` binary at `/usr/local/bin/mise` plus CA certificates | `COPY --from=` source for any base image                       |
| `debian`, `2026.9-debian`, `2026.9.11-debian` | `debian:trixie-slim` with the glibc `mise` binary, `ca-certificates`, `curl`, and `git`     | Running mise directly in CI jobs, devcontainers, or for repros |

Both images set `MISE_DATA_DIR=/mise`, `MISE_CONFIG_DIR=/mise`,
`MISE_CACHE_DIR=/mise/cache`, and put `/mise/shims` on `PATH`. Neither
preinstalls any tool; `mise install` does that for the project. The `2026.9`
style tag follows the latest patch release of that month, and `latest` and
`debian` follow the newest release.

The `dev` tag is a large image built from source for mise's own tooling. It is
not a supported image; do not depend on it.

### Copy the binary into your own image

The scratch image is the source for a single `COPY` line. Its binary is
statically linked, so the same line works on Debian, Alpine, distroless, and
any other base:

```Dockerfile [Dockerfile]
FROM debian:13-slim

COPY --from=ghcr.io/jdx/mise:2026.9.11 /usr/local/bin/mise /usr/local/bin/mise

RUN apt-get update \
    && apt-get -y --no-install-recommends install ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"
```

To pin the exact bytes rather than a tag, reference the image by digest.
`docker buildx imagetools inspect ghcr.io/jdx/mise:2026.9.11` prints it, and
tools such as Renovate and Dependabot can keep a pinned digest current:

```Dockerfile
COPY --from=ghcr.io/jdx/mise@sha256:<digest> /usr/local/bin/mise /usr/local/bin/mise
```

### Use the debian image as a base

The debian image has no `ENTRYPOINT`, so it works as a plain base image and
as a CI job image:

```Dockerfile [Dockerfile]
FROM ghcr.io/jdx/mise:2026.9.11-debian

WORKDIR /app
# Also copy mise.lock if the project has one.
COPY mise.toml ./
RUN mise trust && mise install
COPY . .
CMD ["mise", "exec", "--", "node", "server.js"]
```

Add any OS packages your tools need with `apt-get`; the image only ships
`ca-certificates`, `curl`, and `git`. To reproduce a mise issue in a clean
environment, run it interactively:

```shell
docker run -it --rm ghcr.io/jdx/mise:debian bash
```

## Installing mise yourself

If you would rather not depend on the official images, these methods install a
specific mise version without piping a remote script to a shell.

### Distribution packages

The [apt](/installing-mise.html#apt), [dnf](/installing-mise.html#dnf), and
[apk](/installing-mise.html#apk) repositories verify packages with the
distribution's own signing checks. This Debian example uses
`extrepo`:

```Dockerfile [Dockerfile]
# syntax=docker/dockerfile:1
FROM debian:13-slim

RUN <<EOF
  set -ex
  apt-get update
  apt-get install -y extrepo
  extrepo enable mise
  apt-get remove -y --auto-remove extrepo # extrepo and its deps are not needed after extrepo enable
  apt-get update
  apt-get install -y mise
  rm -fr /var/lib/apt/lists/*
EOF
```

With this approach you cannot choose the mise version with `MISE_VERSION`;
pin it with apt version constraints instead.

### Verified release download

Each release ships `SHASUMS256.txt` signed with minisign and GPG. This
downloads one release binary and checks it against the signed checksums, so
the build fails if either the binary or the checksum file was altered:

```Dockerfile [Dockerfile]
FROM debian:13-slim

ARG MISE_VERSION=2026.9.11
ARG MISE_MINISIGN_KEY=RWTC3g8W3z4RZK3V3qv7fa1QY4JEWyBtqIHW+85QlJpZc5yG+uNYNBSZ

RUN apt-get update \
    && apt-get -y --no-install-recommends install ca-certificates curl git minisign \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    base="https://github.com/jdx/mise/releases/download/v${MISE_VERSION}"; \
    asset="mise-v${MISE_VERSION}-linux-$(dpkg --print-architecture | sed 's/amd64/x64/')"; \
    cd /tmp; \
    curl -fsSLO "$base/SHASUMS256.txt"; \
    curl -fsSLO "$base/SHASUMS256.txt.minisig"; \
    curl -fsSLO "$base/$asset"; \
    minisign -Vm SHASUMS256.txt -P "$MISE_MINISIGN_KEY"; \
    grep " ./$asset\$" SHASUMS256.txt | sha256sum -c --strict; \
    install -m 755 "$asset" /usr/local/bin/mise; \
    rm -f SHASUMS256.txt SHASUMS256.txt.minisig "$asset"
```

The public key above is the mise release key from
[`minisign.pub`](https://github.com/jdx/mise/blob/main/minisign.pub). Use the
`-musl` asset for Alpine and other musl bases.

### Committed wrapper

[`mise generate install-script -l -w`](/cli/generate/install-script.html)
writes a `bin/mise` wrapper whose checksums were verified when it was
generated. Commit it, copy it into the image, and it installs that pinned
version on first use. See
[Continuous integration](/continuous-integration.html#bootstrapping).

### Install script

The `mise.run` installer picks the platform and verifies the download's
checksum itself. Set `MISE_VERSION` to pin the release:

```Dockerfile [Dockerfile]
FROM debian:13-slim

RUN apt-get update  \
    && apt-get -y --no-install-recommends install  \
        # install any other dependencies you might need
        curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV MISE_INSTALL_PATH="/usr/local/bin/mise"
ENV PATH="/mise/shims:$PATH"
# ENV MISE_VERSION="..."

RUN curl --proto '=https' --proto-redir '=https' \
    --fail --show-error --silent --location https://mise.run | sh
```

## Installing project tools

Whichever way mise got into the image, exclude local credentials from the
build context before building:

```gitignore [.dockerignore]
.env
.env.*
*.tfvars
*.tfvars.json
```

To install project tools as a build layer, copy the project config before its
source files:

```Dockerfile
WORKDIR /app
COPY mise.toml ./
RUN mise install
COPY . .
```

Also copy `mise.lock` if the project uses a lockfile, plus any files the config
reads. If an install hook needs application files, copy those before `mise install`.
Keep credentials out of the build context rather than relying on later image layers to remove them.
Use `mise exec -- <command>` or `mise run <task>` in `RUN` and `CMD` instructions;
Docker build shells do not run interactive activation hooks.

## Shared tools in multi-user containers

For toolbox containers or bastion hosts where tools should be pre-installed for all users,
use `mise install --system` to install tools into `/usr/local/share/mise/installs`.
Each user's mise finds these system-level tools automatically without any configuration.

`--system` shares the install location between users; it does not put binaries on `PATH`
for use without mise. If you want tools other users can run with no mise involved, see
[How do I install tools other users can run without mise?](/faq.html#how-do-i-install-tools-other-users-can-run-without-mise)

```Dockerfile [Dockerfile]
FROM ghcr.io/jdx/mise:debian

RUN apt-get update \
    && apt-get -y --no-install-recommends install build-essential \
    && rm -rf /var/lib/apt/lists/*

# Pre-install tools to the system-wide shared directory
RUN mise install --system node@26 python@3.15
```

Users can inspect the shared installations with `mise ls --installed`. The
versions below illustrate the output; patch versions depend on when the image
was built:

```shell
$ mise ls --installed
node    26.0.0 (system)
python  3.15.0 (system)
```

Users can install additional versions in their own directory — those take priority over
system versions. To customize the system directory, set `MISE_SYSTEM_DATA_DIR`.

You can also configure additional shared directories with `MISE_SHARED_INSTALL_DIRS`
(paths separated by `:` on Unix and `;` on Windows) or the `shared_install_dirs` setting.

### Devcontainers with home directory mounts

Devcontainers often mount the user's home directory, which means `~/.local/share/mise/installs`
comes from the mount rather than the Docker image. Tools pre-installed during `docker build`
into `~/.local/share/mise/installs` would be hidden by the mount.

Use `mise install --system` to install tools to `/usr/local/share/mise/installs` instead —
this path is outside `~` and survives home directory mounts:

```Dockerfile [Dockerfile]
FROM debian:13-slim
# ... install mise ...
RUN mise install --system node@26 python@3.15
```

When the container starts with `~` mounted, users still see the system tools automatically.
Any tools they install normally go to `~/.local/share/mise/installs` (on the mount) and
take priority over system versions.

## Overriding libc detection

In minimal Docker images (scratch, busybox, distroless) where no dynamic linker
files exist, mise may not detect whether the system uses musl or glibc. Set `libc`
or `MISE_LIBC` to override the detection:

```Dockerfile
ENV MISE_LIBC=musl
RUN mise install
```

Valid values are `musl`, `glibc`, and `gnu` (case-insensitive, with `gnu` treated
as glibc). Invalid values are silently ignored, and mise falls back to runtime
detection. When the mise binary is compiled for musl (the default for Linux
releases), it also falls back to musl automatically when no linker is detected.

## Task to run mise in a Docker container

This is useful for reproducing a mise issue in a clean environment.

```toml [mise.toml]
[tasks.docker]
interactive = true
run = "docker run -it --rm ghcr.io/jdx/mise:debian bash"
```

Then:

```shell
mise run docker
```

Inside the disposable container, run `mise doctor` or create a small `mise.toml`
that reproduces the issue. Activate mise with `eval "$(mise activate bash)"` only
when testing interactive shell behavior. Exiting the shell removes the container
because the task uses `--rm`.
