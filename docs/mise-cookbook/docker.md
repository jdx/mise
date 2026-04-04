# Mise + Docker Cookbook

Here are some tips on using Docker with mise.

## Docker image with mise

Here is an example Dockerfile showing how to install mise in a Docker image.

```Dockerfile [Dockerfile]
FROM debian:13-slim

RUN apt-get update  \
    && apt-get -y --no-install-recommends install  \
        # install any other dependencies you might need
        sudo curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_DATA_DIR="/mise"
ENV MISE_CONFIG_DIR="/mise"
ENV MISE_CACHE_DIR="/mise/cache"
ENV MISE_INSTALL_PATH="/usr/local/bin/mise"
ENV PATH="/mise/shims:$PATH"
# ENV MISE_VERSION="..."

RUN curl https://mise.run | sh
```

Build and run the Docker image:

```shell
docker build -t debian-mise .
docker run -it --rm debian-mise
```

## Shared tools in multi-user containers

For toolbox containers or bastion hosts where tools should be pre-installed for all users,
use `mise install --system` to install tools into `/usr/local/share/mise/installs`.
Each user's mise will automatically find these system-level tools without any configuration.

```Dockerfile [Dockerfile]
FROM debian:13-slim

RUN apt-get update  \
    && apt-get -y --no-install-recommends install  \
        sudo curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ENV MISE_INSTALL_PATH="/usr/local/bin/mise"

# Install mise
RUN curl https://mise.run | sh

# Pre-install tools to the system-wide shared directory
RUN mise install --system node@22 python@3.13
```

Users in the container will see these tools automatically:

```shell
$ mise ls
node    22.0.0 (system)
python  3.13.0 (system)
```

Users can install additional versions in their own directory — those take priority over
system versions. To customize the system directory, set `MISE_SYSTEM_DATA_DIR`.

You can also configure additional shared directories with `MISE_SHARED_INSTALL_DIRS`
(colon-separated paths) or the `shared_install_dirs` setting.

### Devcontainers with home directory mounts

Devcontainers often mount the user's home directory, which means `~/.local/share/mise/installs`
comes from the mount rather than the Docker image. Tools pre-installed during `docker build`
into `~/.local/share/mise/installs` would be hidden by the mount.

Use `mise install --system` to install tools to `/usr/local/share/mise/installs` instead —
this path is outside `~` and survives home directory mounts:

```Dockerfile [Dockerfile]
FROM debian:13-slim
# ... install mise ...
RUN mise install --system node@22 python@3.13
```

When the container starts with `~` mounted, users still see the system tools automatically.
Any tools they install normally go to `~/.local/share/mise/installs` (on the mount) and
take priority over system versions.

## Overriding libc detection with MISE_LIBC

In minimal Docker images (scratch, busybox, distroless) where no dynamic linker
files exist, mise may not detect whether the system uses musl or glibc. Set `MISE_LIBC`
to force the detection:

```Dockerfile
ENV MISE_LIBC=musl
RUN mise install
```

Valid values are `musl` and `gnu` (case-insensitive). Invalid values are silently
ignored and mise falls back to runtime detection. When the mise binary is compiled
for musl (the default for Linux releases), it will also fall back to musl
automatically when no linker is detected.

## Task to run mise in a Docker container

This can be useful if you need to reproduce an issue you're having with mise in a clean environment.

```toml [mise.toml]
[tasks.docker]
run = "docker run -it --rm debian-mise"
```

Build the image first (see above), then:

```shell
❯ mise docker
[docker] $ docker run -it --rm debian-mise
root@75f179a190a1:/# eval "$(mise activate bash)"
# overwrite configuration and prune to give us a clean state
root@75f179a190a1:/# echo "" > /mise/config.toml
root@75f179a190a1:/# mise prune --yes
# ...
```
