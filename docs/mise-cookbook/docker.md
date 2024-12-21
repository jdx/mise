# Mise + Docker Cookbook

Here are some tips on using Docker with mise.

## Docker image with mise

Here is an example Dockerfile showing how to install mise in a Docker image.

```Dockerfile [Dockerfile]
FROM debian:12-slim

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

## Task to run mise in a Docker container

This can be useful use if you need to reproduce an issue you're having with mise in a clean environment.

```toml [mise.toml]
[tasks.docker]
run = "docker run --pull=always -it --rm --entrypoint bash jdxcode/mise:latest"
```

Example usage:

```shell
❯ mise docker
[docker] $ docker run --pull=always -it --rm --entrypoint bash jdxcode/mise:latest
# latest: Pulling from jdxcode/mise
# Digest: sha256:eecc479b6259479ffca5a4f9c68dbfe8631ca62dc59aa60c9ab5e4f6e9982701
# Status: Image is up to date for jdxcode/mise:latest
root@75f179a190a1:/mise# eval "$(mise activate bash)"
# overwrite configuration and prune to give us a clean state
root@75f179a190a1:/mise# echo "" >/mise/config.toml
root@75f179a190a1:/mise# mise prune --yes
# mise pruned configuration links
# mise python@3.13.1 ✓ remove /mise/cache/python/3.13.1
# ...
```
