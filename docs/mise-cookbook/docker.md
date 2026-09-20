---
description: "Use mise in Docker: choose an official image, pin releases, install project tools, and share tools in development containers."
---

# Docker Cookbook

Use the Debian image to run mise and install project tools, or copy the static
mise binary into an existing image. This cookbook also covers verified downloads
and shared tool installations for development containers.

## Official images

Release images are published to `ghcr.io/jdx/mise` and Docker Hub (`jdxcode/mise`)
for `linux/amd64` and `linux/arm64`. Both registries use the same tags.

| Image       | Example tags                                  | Use it for                                         |
| ----------- | --------------------------------------------- | -------------------------------------------------- |
| Scratch     | `2026.9.11`, `2026.9`, `latest`               | Copying the static musl binary into your own image |
| Debian slim | `2026.9.11-debian`, `2026.9-debian`, `debian` | Running mise in CI jobs and development containers |

The scratch image contains `/usr/local/bin/mise` and CA certificates. It has no
shell or package manager. The Debian image uses `debian:trixie-slim` and includes
the glibc mise binary, `ca-certificates`, `curl`, and `git`. Neither image
preinstalls tools managed by mise.

Both images set `MISE_DATA_DIR=/mise`, `MISE_CONFIG_DIR=/mise`, and
`MISE_CACHE_DIR=/mise/cache`, and add `/mise/shims` to `PATH`. The build verifies
the release binaries against minisign-signed checksums before copying them into
the images.

Use a full version tag to select a release. Month tags such as `2026.9` and
floating tags (`latest` and `debian`) change as images are published. For an
exact image, [pin its digest](#pin-an-image-by-digest).

::: warning Migrating from the previous image
`latest` now selects the scratch image. If you used the previous source-built
image as a CI or development environment, switch to `debian` and install the
tools your project needs. The old image remains available as `dev` for mise's
internal tooling; it is unsupported for general use.
:::

### Copy the binary into your own image

Copy `/usr/local/bin/mise` from the scratch image into your Linux image. The
statically linked binary runs on both glibc and musl systems, including Debian
and Alpine. The destination still needs CA certificates for HTTPS and any
OS dependencies required by your tools:

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

`COPY --from` copies only the binary: it does not inherit the source image's
certificates or environment variables. The example installs certificates and
sets the mise directories explicitly.

### Use the Debian image as a base

The Debian image has no `ENTRYPOINT`, so it works as a base image and lets CI
runners supply their own shell command. This example assumes `mise.toml`
declares Node.js and the application starts with `node server.js`:

```Dockerfile [Dockerfile]
FROM ghcr.io/jdx/mise:2026.9.11-debian

WORKDIR /app
# Also copy mise.lock if the project has one.
COPY mise.toml ./
RUN mise trust && mise install
COPY . .
CMD ["mise", "exec", "--", "node", "server.js"]
```

Add OS packages required by your tools with `apt-get`. For GitLab CI, see the
[CI image example](/continuous-integration.html#use-the-official-image).

### Pin an image by digest

A version tag selects a release, but a rebuilt image can change what that tag
points to. To select an exact image, inspect its digest:

```shell
docker buildx imagetools inspect ghcr.io/jdx/mise:2026.9.11
```

Replace `<digest>` below with the digest from that output:

```Dockerfile
COPY --from=ghcr.io/jdx/mise@sha256:<digest> /usr/local/bin/mise /usr/local/bin/mise
```

The same syntax works in `FROM` and CI image references. Inspect the `-debian`
tag to get the Debian image's digest.

## Installing project tools

Before building, exclude local credentials from the Docker build context with
a `.dockerignore` file. Adapt these patterns to your project:

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
RUN mise trust && mise install
COPY . .
```

Also copy `mise.lock` if the project uses a lockfile, plus any files the config
reads. If an install hook needs application files, copy those before `mise install`.
Keep credentials out of the build context rather than relying on later image layers to remove them.
Use `mise exec -- <command>` or `mise run <task>` in `RUN` and `CMD` instructions;
Docker build shells do not run interactive activation hooks.

## Installing mise yourself

If you need to install mise directly into an existing base image, choose a
package repository, a verified release download, a committed wrapper, or the
install script below.

### Distribution packages

The [apt](/installing-mise.html#apt), [dnf](/installing-mise.html#dnf), and
[apk](/installing-mise.html#apk) installation methods use the package manager's signature verification. This
Debian example uses `extrepo` to configure the mise apt repository:

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

Each release ships `SHASUMS256.txt` signed with minisign and GPG. This Debian
example downloads the glibc binary for `amd64` or `arm64`, verifies the checksum
file with minisign, and checks the binary before installing it:

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
writes a `bin/mise` wrapper from a signature-verified installer with embedded
checksums. Commit the wrapper, copy it into the image, and call `./bin/mise`
to install and run its pinned version on first use. See
[Continuous integration](/continuous-integration.html#bootstrapping).

### Install script

The `mise.run` installer selects the platform and checks the binary's checksum. Set `MISE_VERSION` to pin the release:

```Dockerfile [Dockerfile]
FROM debian:13-slim

RUN apt-get update \
    && apt-get -y --no-install-recommends install \
        # install any other dependencies you might need
        curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV MISE_INSTALL_PATH="/usr/local/bin/mise"
ENV PATH="/mise/shims:$PATH"
ENV MISE_VERSION="2026.9.11"

RUN curl --proto '=https' --proto-redir '=https' \
    --fail --show-error --silent --location https://mise.run | sh
```

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
system versions. When using an official image as the base, override its shared
`MISE_DATA_DIR`, `MISE_CONFIG_DIR`, and `MISE_CACHE_DIR` for each user if you want
private directories. To customize the system directory, set `MISE_SYSTEM_DATA_DIR`.

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
