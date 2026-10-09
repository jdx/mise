---
description: "Use the same mise.toml in CI and development so both environments select the same tools."
---

# Continuous integration

Use the same `mise.toml` in CI and development so both environments select the
same tools. Run commands with `mise exec` or [`mise run`](/tasks/) to load those
tools and the project's [environment variables](/environments/). CI does not
need shell activation.

For reproducible installs, commit a [lockfile](/dev-tools/mise-lock.html) and
install with `mise install --locked`, which fails when a tool has no recorded
download for the platform; see
[strict lockfile mode](/dev-tools/mise-lock.html#strict-lockfile-mode). Pin the
mise version separately if the pipeline also needs to control updates to mise
itself.

## GitHub Actions {#github-actions}

[mise-action](https://github.com/jdx/mise-action) installs mise and the tools
declared in the checked-out repository. By default it also caches the tools,
adds the shims directory to `PATH`, and exports the project's environment
variables to later steps:

```yaml
name: test
on:
  pull_request:
  push:
    branches: [main]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
      - uses: jdx/mise-action@v5
      - run: mise run test
```

::: warning
A restored tool cache is trusted as is: mise does not check installed files against
`mise.lock`. See [Restored caches and installed tools](/dev-tools/mise-lock.html#restored-caches).
:::

The example assumes a `test` task in `mise.toml`, such as
`test = "npm ci && npm test"` under `[tasks]`. `mise exec -- npm test` works as
well.

When the repository has a `mise.lock`, the action runs `mise install --locked`
automatically. Use the `version` input to pin mise, and `working_directory` to
select a subproject. Keep tool versions in the repository config; the
`mise_toml` and `tool_versions` inputs are for workflows that supply their own
config. See the [action inputs](https://github.com/jdx/mise-action/blob/v5/action.yml)
for cache and token options.

The action gives mise the workflow's GitHub token while it installs tools, but
does not export it to later steps. If later steps install tools or hit GitHub
API rate limits, pass a token to them as shown in
[GitHub tokens](/dev-tools/github-tokens.html#ci-github-actions).

To start a workflow, [`mise generate github-action`](/cli/generate/github-action.html)
prints one that runs `mise run ci` on pull requests, tags and pushes to the
current branch. Add `--write` to save it as `.github/workflows/ci.yml`, and
review the triggers and action versions before committing it:

```sh
mise generate github-action --write --task=ci
```

## GitLab CI {#gitlab-ci}

Both examples assume an amd64 runner (change the cache prefix for arm64), a
Node.js project with a `build` script in `package.json`, and a committed
`mise.lock`. Without a lockfile, drop `mise.lock` from the cache key and
`--locked` from the install. GitLab accepts at most two paths in
`cache:key:files` by default. Add any OS packages your tools need to
`before_script`, or build a CI image that has them.

### Use a committed wrapper {#use-a-committed-wrapper}

This job runs the [committed wrapper](#bootstrapping) on a Debian image. The
localized wrapper keeps mise's directories under `.mise/`, which the job
caches:

```yaml
build-job:
  stage: build
  image: debian:13-slim
  cache:
    key:
      prefix: mise-debian13-amd64
      files: [mise.toml, mise.lock]
    paths:
      - .mise/installs/
      - .mise/cache/
  before_script:
    - apt-get update && apt-get install -y --no-install-recommends curl ca-certificates tar
  script:
    - ./bin/mise install --locked
    - ./bin/mise exec -- npm ci
    - ./bin/mise exec -- npm run build
```

To start a new cache when you regenerate the wrapper for a newer mise, change
the prefix, for example to `mise-debian13-amd64-2026.10.4`.

### Use the official image {#use-the-official-image}

The [official `-debian` image](/mise-cookbook/docker.html#official-images)
already contains mise, `curl`, `git` and CA certificates, so a job can use it
directly, without a wrapper or an `ENTRYPOINT` override. Set the mise data and
cache directories inside the project so GitLab can cache them:

```yaml
build-job:
  stage: build
  image: ghcr.io/jdx/mise:2026.10.4-debian
  variables:
    MISE_DATA_DIR: $CI_PROJECT_DIR/.mise
    MISE_CACHE_DIR: $CI_PROJECT_DIR/.mise/cache
  cache:
    key:
      prefix: mise-image-amd64
      files: [mise.toml, mise.lock]
    paths:
      - .mise/installs/
      - .mise/cache/
  script:
    - mise install --locked
    - mise exec -- npm ci
    - mise exec -- npm run build
```

Replace the image's version with the mise release you want to pin.

## Other CI providers {#any-ci-provider}

On other providers, commit a wrapper script that downloads a pinned mise release
on first use, then call it for every mise command. The runner needs `bash`,
`curl`, CA certificates, `tar`, and either `sha256sum` or `shasum` to download,
verify and extract mise.

### Generate the wrapper {#bootstrapping}

Generate it locally with
[`mise generate install-script`](/cli/generate/install-script.html):

```sh
mise generate install-script --localize --write
```

This writes `bin/mise`. Commit it and add `.mise/` to `.gitignore`. The
localized wrapper keeps its mise binary, installed tools, cache and state under
`.mise/`, trusts the project's config files and ignores the user's global
config. Add `--windows` to also write `bin/mise.cmd` for Windows runners and
contributors; it requires `--write`.

Run the wrapper for both installing and running, so that both use those
directories. This example assumes a Node.js project with a committed
`package-lock.json` and a `test` script:

```sh
set -eu
./bin/mise install
./bin/mise exec -- npm ci
./bin/mise exec -- npm test
```

Use `./bin/mise install --locked` when the repository has a lockfile. On
Windows runners, run `.\bin\mise.cmd` instead.

The wrapper pins the mise release that `mise self-update` would select when you
generate it (the newest stable release older than
[`self_update.minimum_release_age`](/configuration/settings.html#self_update.minimum_release_age),
24h by default), or the release passed with `--version`. Regenerate and commit
the wrapper to move to a newer release, or set `MISE_VERSION` when running it.
`MISE_INSTALL_PATH` overrides where it keeps the binary. Without `--localize`,
the wrapper uses the normal mise directories and keeps its binary under the
data directory's `bootstrap/` subdirectory.

Wrappers generated by older mise versions keep the binary in the cache
directory, where `mise cache clear` or cache pruning deletes it; regenerate
them. A regenerated wrapper reuses an existing cache-directory binary of the
same version instead of downloading it again.

If jobs install tools from private GitHub repositories or hit API rate limits,
give mise a token; see [GitHub tokens](/dev-tools/github-tokens.html).

## Xcode Cloud {#xcode-cloud}

Use an Xcode Cloud
[post-clone script](https://developer.apple.com/documentation/xcode/writing-custom-build-scripts)
at `ci_scripts/ci_post_clone.sh` to install and run tools before the build.
Commit the [generated wrapper](#bootstrapping) at `bin/mise`. This example
assumes SwiftLint is declared in the repository's `mise.toml`:

```sh
#!/bin/sh
set -eu
cd "$CI_PRIMARY_REPOSITORY_PATH"
./bin/mise install
./bin/mise exec -- swiftlint lint
```

Make the script executable before committing it. Environment changes in this
script do not reach later build phases, so use `mise exec` in any other phase
that needs mise tools. For local Xcode builds, see
[Editors and IDEs](/ide-integration.html#xcode).

## Caching {#caching}

Cache installed tools to avoid downloading them on every job. Cache
`$MISE_DATA_DIR/installs` (by default `~/.local/share/mise/installs`) and
`$MISE_CACHE_DIR`; with a localized wrapper these are `.mise/installs` and
`.mise/cache`. Include the runner's OS and architecture, `mise.toml` and the
lockfile in the cache key, and use separate caches for jobs that use different
[config environments](/configuration/environments.html) or install options.

Still run `mise install` after restoring a cache: it installs anything missing,
and the job must also succeed with an empty cache. See
[Directories](/directories.html) for where mise keeps installs and metadata.

## Trust and untrusted branches {#safe-mode}

When mise detects a CI environment, it trusts every config file without
prompting unless [paranoid mode](/paranoid.html) is on, so tasks, hooks and
`[env]` from the checked-out branch run normally. A job that reads branches you
did not write while holding secrets, such as a bot that resolves versions from
pull requests, should set `MISE_SAFE=1` so project config cannot run code or
inject environment variables:

```sh
MISE_SAFE=1 mise lock --bump --dry-run --json
```

Remove `--dry-run` when the bot should update `mise.lock`. Safe mode refuses
tasks, template `exec()` and plugin installation, ignores project environment
variables and settings, and skips hooks. Some backends need to run code and
cannot resolve versions in this mode. Operator-owned global config still
applies. See [Safe mode](/security.html#safe-mode) for the full list and the
backend restrictions.
