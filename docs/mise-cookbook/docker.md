# Docker Cookbook

Install mise inside an image, use it to run project commands, or preinstall tools
outside user home directories for shared development containers. Building these
examples requires Docker and a running container engine.

## Docker image with mise

Here is an example Dockerfile showing how to install mise in a Docker image.

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

RUN curl --fail --show-error --silent --location https://mise.run | sh
```

Build and run the Docker image:

```shell
docker build -t debian-mise .
docker run -it --rm debian-mise
```

The image above installs mise itself. To install project tools as a build layer,
copy the project config before its source files:

```Dockerfile
WORKDIR /app
COPY mise.toml ./
RUN mise install
COPY . .
```

Also copy `mise.lock` if the project uses a lockfile, plus any files the config
reads. If an install hook needs application files, copy those before `mise install`.
Use `mise exec -- <command>` or `mise run <task>` in `RUN` and `CMD` instructions;
Docker build shells do not run interactive activation hooks.

## Shared tools in multi-user containers

For toolbox containers or bastion hosts where tools should be pre-installed for all users,
use `mise install --system` to install tools into `/usr/local/share/mise/installs`.
Each user's mise finds these system-level tools automatically without any configuration.

`--system` shares the install location between users; it does not put binaries on `PATH`
for use without mise. If you want tools other users can run with no mise involved, see
[How do I install tools other users can run without mise?](/faq.html#how-do-i-install-tools-other-users-can-run-without-mise)

The following example also shows installing mise with `extrepo` on a Debian/Ubuntu image.
With this approach, you cannot specify `MISE_VERSION` or `MISE_INSTALL_PATH`.

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
  apt-get install -y mise build-essential
  rm -fr /var/lib/apt/lists/*
EOF

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
run = "docker run -it --rm debian-mise"
```

Build the image first (see above), then:

```shell
mise run docker
```

Inside the disposable container, run `mise doctor` or create a small `mise.toml`
that reproduces the issue. Activate mise with `eval "$(mise activate bash)"` only
when testing interactive shell behavior. Exiting the shell removes the container
because the task uses `--rm`.
