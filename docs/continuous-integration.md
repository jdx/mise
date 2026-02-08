# Continuous integration

You can use Mise in continuous integration environments to provision the environment with the tools the project needs.
We recommend that your project pins the tools to a specific version to ensure the environment is reproducible.

## Any CI provider

Continuous integration pipelines allow running arbitrary commands. You can use this to install Mise and run `mise install` to install the tools:

```yaml
script: |
  curl https://mise.run | sh
  mise install
```

To ensure you run the version of the tools installed by Mise, make sure you run them through the `mise x` command:

```yaml
script: |
  mise x -- npm test
```

Alternatively, you can add the [shims](/dev-tools/shims.md) directory to your `PATH`, if the CI provider allows it.

### Bootstrapping

An alternative to calling `curl https://mise.run | sh` is to use [`mise generate bootstrap`](/cli/generate/bootstrap.html) to generate a script that runs and install `mise`.

```shell
mise generate bootstrap -l -w
```

Add the `.mise/` to your `.gitignore` and commit the generated `./bin/mise` file. You can now use `./bin/mise` to install and run `mise` directly in CI.

```yaml
script: |
  ./bin/mise install
  ./bin/mise x -- npm test
```

## GitHub Actions

If you use GitHub Actions, we provide a [mise-action](https://github.com/jdx/mise-action) that wraps the installation of Mise and the tools. All you need to do is to add the action to your workflow:

```yaml
name: test
on:
  pull_request:
    branches:
      - main
  push:
    branches:
      - main
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v3
        with:
          version: 2024.12.14 # [default: latest] mise version to install
          install: true # [default: true] run `mise install`
          cache: true # [default: true] cache mise using GitHub's cache
          experimental: true # [default: false] enable experimental features
          # automatically write this mise.toml file
          mise_toml: |
            [tools]
            shellcheck = "0.9.0"
          # or, if you prefer .tool-versions:
          tool_versions: |
            shellcheck 0.9.0
      - run: shellcheck scripts/*.sh
```

## GitLab CI

You can use any docker image with `mise` installed to run your CI jobs.
Here's an example using `debian-slim` as base image:
::: details Example Dockerfile

```dockerfile
FROM debian:12-slim

RUN apt-get update  \
    && apt-get -y --no-install-recommends install  \
      # install any tools you need
      sudo curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

RUN curl https://mise.run | MISE_VERSION=v... MISE_INSTALL_PATH=/usr/local/bin/mise sh
```

:::

When configuring your job, you can cache some of the [Mise directories](/directories).

```yaml
build-job:
  stage: build
  image: mise-debian-slim # Use the image you created
  variables:
    MISE_DATA_DIR: $CI_PROJECT_DIR/.mise/mise-data
  cache:
    - key:
        prefix: mise-
        files: ["mise.toml", "mise.lock"] # mise.lock is optional, only if using `lockfile = true`
      paths:
        - $MISE_DATA_DIR
  script:
    - mise install
    - mise exec --command 'npm build'
```

### Example with the bootstrap script

An alternative is to use [`mise generate bootstrap`](/cli/generate/bootstrap.html) to easily [bootstrap](#bootstrapping) `mise` on GitLab CI.

```
mise generate bootstrap -l -w
```

You can now use a generic docker image such as this one to run and install `mise` in CI.

::: details Example Dockerfile

```dockerfile
FROM debian:12-slim

RUN apt-get update  \
    && apt-get -y --no-install-recommends install sudo curl git ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*
```

:::

Here's an example of a `.gitlab-ci.yml` file:

```yaml
.mise-cache: &mise-cache
  key:
    prefix: mise-
    files: ["mise.toml", "./bin/mise"]
  paths:
    - .mise/installs
    - .mise/mise-2025.1.3

build-job:
  stage: build
  image: my-debian-slim-image # Use the image you created
  cache:
    - <<: *mise-cache
      policy: pull-push
  script:
    - ./bin/mise install
    - ./bin/mise exec --command 'npm build'
```

## Xcode Cloud

If you are using Xcode Cloud, you can use custom `ci_post_clone.sh` [build script](https://developer.apple.com/documentation/xcode/writing-custom-build-scripts) to install Mise. Here's an example:

```bash
#!/bin/sh
curl https://mise.run | sh
export PATH="$HOME/.local/bin:$PATH"

mise install # Installs the tools in mise.toml
eval "$(mise activate bash --shims)" # Adds the activated tools to $PATH

swiftlint {args}
```
