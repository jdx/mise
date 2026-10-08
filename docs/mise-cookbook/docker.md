---
description: "Run mise in Docker: choose an official image, install project tools during a build, and share tools between users."
socialDescription: "Run mise in Docker with the official images, build-time installs, and shared tools."
---

# Docker

Run mise in a container in one of two ways: build on the `-debian` image, which
has mise, `curl` and `git` ready for CI jobs and development containers, or copy
the static binary from the scratch image into an image you already use. Either
way, `mise install` installs the project's tools during the build.

## Official images {#official-images}

Starting with 2026.9.11, images are published for every stable release to
`ghcr.io/jdx/mise` and Docker Hub (`jdxcode/mise`) for `linux/amd64` and
`linux/arm64`. Both registries use the same tags.

| Image       | Example tags                                   | Use it for                                         |
| ----------- | ---------------------------------------------- | -------------------------------------------------- |
| Scratch     | `2026.10.4`, `2026.10`, `latest`               | Copying the static musl binary into your own image |
| Debian slim | `2026.10.4-debian`, `2026.10-debian`, `debian` | Running mise in CI jobs and development containers |

Replace the example version with the
[release](https://github.com/jdx/mise/releases) you want to pin.

The scratch image contains `/usr/local/bin/mise`, which is its entrypoint, and
CA certificates. It has no shell or package manager. The Debian image is based
on Debian 13 (trixie) slim and includes the glibc mise binary,
`ca-certificates`, `curl` and `git`. It has no `ENTRYPOINT`, so it works as a
base image and CI runners can supply their own shell command. Neither image
preinstalls tools.

Both images set `MISE_DATA_DIR=/mise`, `MISE_CONFIG_DIR=/mise` and
`MISE_CACHE_DIR=/mise/cache`, and add `/mise/shims` to `PATH`. The release
workflow verifies the binaries against minisign-signed checksums before copying
them into the images.

Use a full version tag to select a release. Month tags such as `2026.10` and
the floating tags `latest` and `debian` move as new releases are published.
Version tags before 2026.9.11, and month tags up to `2026.8`, still point to
the [previous image](#migrating-from-the-previous-image) and have no `-debian`
variant.

### Pin an image by digest {#pin-an-image-by-digest}

A version tag selects a release, but a rebuilt image can change what that tag
points to. To select an exact image, inspect its digest:

```sh
docker buildx imagetools inspect ghcr.io/jdx/mise:2026.10.4
```

Replace `<digest>` below with the digest from that output:

```Dockerfile
COPY --from=ghcr.io/jdx/mise@sha256:<digest> /usr/local/bin/mise /usr/local/bin/mise
```

The same syntax works in `FROM` and CI image references. Inspect the `-debian`
tag to get the Debian image's digest.

## Install project tools in an image {#installing-project-tools}

Copy the project's config before its source files and run `mise install` in its
own layer, so Docker rebuilds the tools only when the config changes.

### Build on the Debian image {#use-the-debian-image-as-a-base}

This example assumes `mise.toml` declares Node.js and the application starts
with `node server.js`:

```Dockerfile [Dockerfile]
FROM ghcr.io/jdx/mise:2026.10.4-debian

WORKDIR /app
# Also copy mise.lock if the project has one.
COPY mise.toml ./
RUN mise trust && mise install
COPY . .
CMD ["mise", "exec", "--", "node", "server.js"]
```

Add OS packages your tools need with `apt-get`. For GitLab CI, see the
[CI image example](/continuous-integration.html#use-the-official-image).

### Copy the binary into your own image {#copy-the-binary-into-your-own-image}

Copy `/usr/local/bin/mise` from the scratch image into your Linux image. The
statically linked binary runs on glibc and musl systems, including Debian and
Alpine. `COPY --from` copies only the binary, not the source image's
certificates or environment variables, so install CA certificates and set the
mise directories yourself, along with any OS packages your tools need:

```Dockerfile [Dockerfile]
FROM debian:13-slim

COPY --from=ghcr.io/jdx/mise:2026.10.4 /usr/local/bin/mise /usr/local/bin/mise

RUN apt-get update \
    && apt-get -y --no-install-recommends install ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"

WORKDIR /app
COPY mise.toml ./
RUN mise trust && mise install
COPY . .
```

### What to copy into the build {#build-context}

Copy `mise.lock` along with `mise.toml` when the project uses a lockfile, plus
any files the config reads. If an install hook needs application files, copy
those before `mise install`.

Exclude local credentials and mise's personal config files from the build
context with a `.dockerignore`, so `COPY . .` does not bake them into the image.
The `.local.toml` files hold personal overrides, which may include secrets.
Adapt these patterns to your project:

```gitignore [.dockerignore]
.env
.env.*
*.tfvars
*.tfvars.json
# mise.local.toml, mise.<env>.local.toml, .mise/config.local.toml and the like
**/*.local.toml
```

Docker build shells do not run interactive activation hooks. Use
`mise exec -- <command>` or `mise run <task>` in `RUN` and `CMD` instructions.

## Install mise without the official images {#installing-mise-yourself}

To install mise into an existing base image yourself, pick a method:

| Method                                                  | Use it when                                                                                |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| [packslip](#bootstrap-with-packslip)                    | You want to pin the installer by digest and choose the mise version with a build argument. |
| [Distribution packages](#distribution-packages)         | The image already trusts your distribution's package repositories.                         |
| [Verified release download](#verified-release-download) | You want an exact version with only `curl` and `minisign`.                                 |
| [Committed wrapper](#committed-wrapper)                 | The repository already commits `bin/mise`.                                                 |
| [Install script](#install-script)                       | You want the shortest Dockerfile.                                                          |

### Bootstrap with packslip {#bootstrap-with-packslip}

[packslip](/installing-mise.html#packslip) checks the release's signature and
publisher before it extracts mise, so the build runs no install script. Pin
packslip by digest and choose the mise version with a build argument: the
installer stays fixed while mise can move to new releases.

This example pins packslip 1.5.1 by its multi-platform image digest and copies
its CA bundle for HTTPS. packslip downloads and extracts the archive itself, so
the image needs no `curl`, `tar` or package manager for this step:

```Dockerfile [Dockerfile]
FROM ghcr.io/jdx/packslip:1.5.1@sha256:fcbbcb85ab02d433d6108c212ffc7eaeda0bbafca4b82111c452568ac680b9c4 AS bootstrap
FROM debian:13-slim

COPY --from=bootstrap /packslip /usr/local/bin/packslip
COPY --from=bootstrap /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

ARG MISE_VERSION=latest
RUN packslip install github.com/jdx/mise --version "$MISE_VERSION" \
      --pin ps1_nlhmwtfeufglxv5myvwvronk7a \
    && mise --version

ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV PATH="/mise/shims:$PATH"

CMD ["mise", "--version"]
```

The digest pins packslip for both `linux/amd64` and `linux/arm64`. The packslip
version, the mise version and the signer pin are independent: the signer pin
identifies mise's GitHub repository and keeps authenticating mise as it
publishes new releases, without fixing a release. `MISE_VERSION=latest` lets
mise move to new releases while packslip stays fixed, which suits a base image,
a CI bootstrap or a distribution package that should not need a new installer
for each mise release. To fix mise as well, pass
`--build-arg MISE_VERSION=2026.10.4`.

Build from a directory that contains this Dockerfile:

```sh
docker build --no-cache -t mise-bootstrap .
docker run --rm mise-bootstrap
```

Docker caches the `RUN` layer, so a build that asks for `latest` keeps the
cached mise until you build with `--no-cache`. packslip does not update itself
or mise, and a running container never updates mise on its own; rebuild, or run
`mise self-update`. You can keep the same packslip digest across mise releases,
but a
[security or format change](https://packslip.dev/docs/compatibility/#maintaining-packaged-verifiers)
may require a newer packslip.

As root, packslip installs mise under `/opt/packslip` and links
`/usr/local/bin/mise` to it. To copy the installation into another stage, copy
`/opt/packslip` as well as the link. Continue with
[installing project tools](#installing-project-tools).

### Distribution packages {#distribution-packages}

The [apt](/installing-mise.html#apt), [dnf](/installing-mise.html#dnf) and
[apk](/installing-mise.html#apk) methods rely on the package manager's signature
checks. This Debian example uses `extrepo` to configure the mise apt repository:

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

`MISE_VERSION` does not choose the version here; pin it with apt version
constraints instead.

### Verified release download {#verified-release-download}

Each release ships `SHASUMS256.txt` signed with minisign and GPG. This Debian
example downloads the glibc binary for `amd64` or `arm64`, checks the checksum
file with minisign, and checks the binary before installing it:

```Dockerfile [Dockerfile]
FROM debian:13-slim

ARG MISE_VERSION=2026.10.4
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

### Committed wrapper {#committed-wrapper}

[`mise generate install-script --localize --write bin/mise`](/cli/generate/install-script.html)
writes a `bin/mise` wrapper from a signature-checked installer with embedded
checksums. Commit the wrapper, copy it into the image, and call `./bin/mise`; it
installs its pinned mise release on first use. See
[Continuous integration](/continuous-integration.html#bootstrapping).

### Install script {#install-script}

The `mise.run` installer selects the platform and checks the binary's checksum.
Set `MISE_VERSION` to pin the release:

```Dockerfile [Dockerfile]
FROM debian:13-slim

RUN apt-get update \
    && apt-get -y --no-install-recommends install curl ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV MISE_INSTALL_PATH="/usr/local/bin/mise"
ENV PATH="/mise/shims:$PATH"
ENV MISE_VERSION="2026.10.4"

RUN curl --proto '=https' --proto-redir '=https' \
    --fail --show-error --silent --location https://mise.run | sh
```

Add OS packages your tools need, such as `build-essential` for tools built from
source.

## Share tools between users {#shared-tools-in-multi-user-containers}

For toolbox containers or bastion hosts where every user should find a set of
tools already installed, run `mise install --system`. It installs into
`/usr/local/share/mise/installs`, which every user's mise checks without a
[`shared_install_dirs`](/configuration/settings.html#shared_install_dirs)
setting. [System installs](/dev-tools/system-installs.html) covers the command
in full.

Installing does not select a version. A project's `mise.toml` (for example
`node = "26"`) or `mise exec node@26 -- node --version` still has to request
it; mise then uses the system copy instead of downloading it again. `--system`
shares the install location; it does not put binaries on `PATH` for use without
mise. For tools other users can run with no mise involved, see the
[FAQ](/faq.html#mise-is-for-dev-tools-not-applications-or-system-packages).

The official images point `MISE_DATA_DIR`, `MISE_CONFIG_DIR` and
`MISE_CACHE_DIR` at `/mise` for every user, so if you base a multi-user image on
them, unset or override those variables for each user. This example avoids that
by copying mise into a plain Debian image, so each user keeps a private
`~/.local/share/mise` and `/usr/local/share/mise/installs` is the shared layer:

```Dockerfile [Dockerfile]
FROM debian:13-slim

COPY --from=ghcr.io/jdx/mise:2026.10.4 /usr/local/bin/mise /usr/local/bin/mise

RUN apt-get update \
    && apt-get -y --no-install-recommends install ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Pre-install tools to the system-wide shared directory
RUN mise install --system node@26 python@3.14
```

Users can inspect the shared installations with `mise ls --installed`. Patch
versions depend on when the image was built:

```text
node    26.x.x (system)
python  3.14.x (system)
```

Users can still install other versions into their own data directory, and those
take priority over the system versions. To move the shared directory or add
other shared directories, see
[System installs](/dev-tools/system-installs.html#directories).

### Pre-install tools for devcontainers {#devcontainers-with-home-directory-mounts}

To start from a generated configuration, run `mise generate devcontainer --write`.
It writes `.devcontainer/devcontainer.json` with the mise dev container feature;
`--mount-mise-data` adds a volume for mise's data directory. See
[`mise generate devcontainer`](/cli/generate/devcontainer.html).

Dev containers often mount the user's home directory, so
`~/.local/share/mise/installs` comes from the mount and hides any tools that
`docker build` installed there. Install the tools with `mise install --system`
instead. They go to `/usr/local/share/mise/installs`, outside `~`, so the mount
does not hide them:

```Dockerfile [Dockerfile]
FROM debian:13-slim
# ... install mise ...
RUN mise install --system node@26 python@3.14
```

When the container starts with `~` mounted, configured versions still resolve to
the system installs. Tools a user installs later go to
`~/.local/share/mise/installs` on the mount and take priority over the system
versions.

## Override libc detection {#overriding-libc-detection}

In minimal images (scratch, busybox, distroless) without dynamic linker files,
mise may not detect whether the system uses musl or glibc. Set the
[`libc`](/configuration/settings.html#libc) setting or `MISE_LIBC` to override
the detection:

```Dockerfile
ENV MISE_LIBC=musl
RUN mise install
```

Valid values are `musl`, `glibc` and `gnu` (case-insensitive, with `gnu` treated
as glibc). mise ignores invalid values and falls back to runtime detection. The
static `-musl` mise build, which the scratch image ships, falls back to musl
when no linker is detected.

## Reproduce an issue in a clean container {#task-to-run-mise-in-a-docker-container}

Start a disposable container from the Debian image to check whether a problem
reproduces outside your machine:

```sh
docker run -it --rm ghcr.io/jdx/mise:debian bash
```

Inside it, run `mise doctor` or create a small `mise.toml` that reproduces the
issue. Activate mise with `eval "$(mise activate bash)"` only when testing
interactive shell behavior. Exiting the shell removes the container.

To repeat this often, save it as a task:

```toml [mise.toml]
[tasks.docker]
interactive = true
run = "docker run -it --rm ghcr.io/jdx/mise:debian bash"
```

## Migrate from the old latest image {#migrating-from-the-previous-image}

`latest` selects the scratch image, which has no shell. If a CI job or
development container used the older source-built `latest` image, switch to the
`debian` tag and install your project's tools with `mise install`. The old
image is still published as `dev` for mise's own tooling and is not supported
for other use.
