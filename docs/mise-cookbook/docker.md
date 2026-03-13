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
