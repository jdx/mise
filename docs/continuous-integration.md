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
      - uses: actions/checkout@v4
      - uses: jdx/mise-action@v2
        with:
          version: 2023.12.0 # [default: latest] mise version to install
          install: true # [default: true] run `mise install`
          cache: true # [default: true] cache mise using GitHub's cache
          # automatically write this .tool-versions file
          experimental: true # [default: false] enable experimental features
          tool_versions: |
            shellcheck 0.9.0
          # or, if you prefer mise.toml format:
          mise_toml: |
            [tools]
            shellcheck = "0.9.0"
      - run: shellcheck scripts/*.sh
```

## GitLab CI

You can use any docker image with `mise` installed to run your CI jobs.
Here's an example using `debian-slim` as base image:
::: details

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
    MISE_DATA_DIR: .mise/mise-data
    MISE_CACHE_DIR: .mise/mise-cache
  cache:
    - key:
        prefix: mise-
        files: ["mise.toml", "mise.lock"] # mise.lock is optional, only if using `lockfile = true`
      paths:
        - $MISE_DATA_DIR
        - $MISE_CACHE_DIR
  script:
    - mise install
    - mise exec --command 'npm build'
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
