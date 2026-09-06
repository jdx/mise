# Continuous integration

Use the same `mise.toml` in CI and development so both environments select the same tools.
Run commands with `mise exec` or [`mise run`](/tasks/) to load those tools and the project's
[environment variables](/environments/). Interactive shell activation is not needed in CI.

For reproducible installs, commit a [lockfile](/dev-tools/mise-lock.html) and run
`mise install --locked`. Pin the mise version separately if your pipeline also needs to
control updates to mise itself.

## Any CI provider

The following shell commands use the [committed wrapper](#bootstrapping) and run from the
checked-out repository. They assume a Node.js
project with Node declared in `mise.toml`, a committed `package-lock.json`, and a `test`
script in `package.json`. Replace the npm commands with your project's build or test commands.
The runner needs `curl`, CA certificates, `tar`, and either `sha256sum` or `shasum`
to download, verify, and extract mise.

```sh
set -eu
./bin/mise install
./bin/mise exec -- npm ci
./bin/mise exec -- npm test
```

Set `MISE_VERSION` when running the wrapper to select a mise release. Use
`./bin/mise install --locked` instead of `./bin/mise install` when the repository has a lockfile.

### Bootstrapping

A committed wrapper can install mise on demand, avoiding a separate installation step in each
pipeline. Generate it locally with [`mise generate install-script`](/cli/generate/install-script.html):

```sh
mise generate install-script -l -w
```

Commit the generated `bin/mise` file and add `.mise/` to `.gitignore`. The localized wrapper
keeps its mise binary, installed tools, cache, and state under `.mise/`. Use the wrapper for
both installation and execution so both commands use those directories:

```sh
./bin/mise install
./bin/mise exec -- npm ci
./bin/mise exec -- npm test
```

The wrapper defaults to the mise version that generated it. Regenerate and commit the wrapper
to update that default, or set `MISE_VERSION` in CI. `MISE_INSTALL_PATH` overrides the binary's
location. Without `-l`, the wrapper uses the normal mise directories and keeps its binary
under the data directory's `bootstrap/` subdirectory. Older wrappers may reuse a binary from
the previous cache-directory location; regenerate them to adopt current installation behavior.

### Caching

Cache installed tools to avoid downloading them on every job. Include the runner's OS and
architecture, mise configuration, and lockfile in the cache key. Separate caches for jobs
that use different [environments](/configuration/environments.html) or installation options.
Still run `mise install` after restoring a cache: it fills in missing tools.

See [directories](/directories.html) for the locations of installs and metadata. A cache is
an optimization; the job should also succeed with an empty cache.

## Running against untrusted config (safe mode)

A bot that resolves versions from pull request branches can use `MISE_SAFE=1` to prevent project
configuration from executing code or injecting environment variables. For example:

```sh
MISE_SAFE=1 mise lock --bump --dry-run --json
```

Remove `--dry-run` when the bot should update `mise.lock`. Safe mode rejects operations such
as tasks, template `exec()`, and plugin installation; it ignores project environment/settings
and suppresses hooks. Some backends need code execution and cannot resolve versions in this
mode. Operator-owned global configuration still applies. See [Safe mode](/security.html#safe-mode)
for the exact boundary and backend restrictions.

## GitHub Actions

The [mise-action](https://github.com/jdx/mise-action) installs mise and the tools declared in
the checked-out repository. By default it also caches tools, adds shims to `PATH`, and exports
mise environment variables for subsequent steps.

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
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v4
      - run: mise exec -- npm ci
      - run: mise exec -- npm test
```

For a repository with `mise.lock`, add `install_args: --locked` under the action's `with` block.
Use the `version` input to pin mise and `working_directory` to select a subproject. Keep tool
versions in the repository configuration; the `mise_toml` and `tool_versions` inputs are useful
when a workflow intentionally supplies its own configuration. See the
[action inputs](https://github.com/jdx/mise-action/tree/v4#inputs) for cache and authentication options.

## GitLab CI

This `.gitlab-ci.yml` uses a Debian image and the [committed wrapper](#bootstrapping). It assumes
the same Node.js project as the generic example above. Add any OS packages required by your
tools to `before_script`, or build a CI image with those packages already installed.

```yaml
build-job:
  stage: build
  image: debian:13-slim
  cache:
    key:
      prefix: mise-debian13-amd64
      files: [bin/mise, mise.toml, mise.lock]
    paths:
      - .mise/installs/
      - .mise/cache/
  before_script:
    - apt-get update && apt-get install -y --no-install-recommends curl ca-certificates tar
  script:
    - ./bin/mise install
    - ./bin/mise exec -- npm ci
    - ./bin/mise exec -- npm run build
```

This example's cache prefix assumes an amd64 runner; choose a distinct prefix for another
architecture. Remove `mise.lock` from the key if the project does not have one, or switch the
install command to `mise install --locked` if it does. The example also requires a `build`
script in `package.json`. The localized wrapper sets the mise directories used by the cache.

## Xcode Cloud

Use an Xcode Cloud [post-clone script](https://developer.apple.com/documentation/xcode/writing-custom-build-scripts)
at `ci_scripts/ci_post_clone.sh` to install and run tools before the build. Commit the
[generated wrapper](#bootstrapping) at `bin/mise`. This example assumes SwiftLint is declared
in the repository's `mise.toml`:

```sh
#!/bin/sh
set -eu
cd "$CI_PRIMARY_REPOSITORY_PATH"
./bin/mise install
./bin/mise exec -- swiftlint lint
```

Make the script executable before committing it. Environment changes in this script do not
configure every later build phase; use `mise exec` in other phases that need mise tools too.
For local Xcode builds, see [IDE integration](/ide-integration.html#xcode).
