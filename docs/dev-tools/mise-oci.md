---
description: Build container images from a project's mise.toml with one OCI layer per tool, then run them locally or push them to a registry.
socialDescription: Turn a project's mise.toml into a container image with one layer per tool.
---

# OCI images <Badge type="warning" text="experimental" />

`mise oci build` packages the tools a project's `mise.toml` declares into a
container image with one [OCI](https://github.com/opencontainers/image-spec)
layer per tool, so changing one tool's version rebuilds only that tool's layer.
`mise oci run` runs a command in the image with Docker or Podman, and
`mise oci push` uploads the image to a registry.

::: warning Experimental
`mise oci` is experimental. Enable it with `experimental = true` under
`[settings]`, or with `MISE_EXPERIMENTAL=1`. Flags and the output layout can
change between releases.
:::

## Requirements

- A Linux host with the image's architecture. mise copies the tools installed on
  the host into the image and does not cross-compile them, so an image built on
  macOS or Windows contains host binaries that fail with `Exec format error`, and
  mise warns when you build there. On another OS, run mise inside a Linux
  container that has its own tool installations; a stock `debian` image does not
  contain mise, and mounting your macOS or Windows installations into it does
  not work.
- The tools installed on the host. Run `mise install` before building: the build
  fails for a configured version that is not installed. The exception is a tool
  layer that [`mise oci push` reuses from the registry](#reuse-layers-from-the-registry).
- Docker or Podman, for `mise oci run` only. Building and pushing use mise's own
  registry client.
- A base image with a compatible libc. See [Base images](#registry-base-image-support).

## Quick start

On Linux, start with a project config such as:

```toml [mise.toml]
[settings]
experimental = true

[tools]
node = "24"
```

Install the tools, build the image, and run a command in it:

```sh
mise install
mise oci build -o ./mise-oci
mise oci run --image-dir ./mise-oci -- node --version
# v24.x.x
```

`mise oci build` writes an OCI image layout to `./mise-oci`; add `mise-oci/` to
`.gitignore`. The image holds your tools on top of the base image set by
[`oci.default_from`](/configuration/settings.html#oci.default_from), but not your
application or its packages. Mount the project with `--volume` for development,
or [copy files into the image](#copy-files-into-the-image).

To inspect the layout without a container engine, run
`skopeo inspect oci:./mise-oci`. To publish the image, see
[Push to a registry](#mise-oci-push).

## What goes into the image

By default the image contains only what the project's config files declare: the
project's `mise.toml` and config files in its parent directories, such as a
monorepo root, but not a `mise.toml` directly in your home directory. Tools,
`[oci]` settings, `[bootstrap.packages]`, and `[dotfiles]` from your global
config (`~/.config/mise/config.toml`) and the system config are left out, as are
`MISE_<TOOL>_VERSION` overrides in the environment, so your personal tools stay
out of a project image. Pass `--include-global` to `build`, `run`, or `push` to
include them. Without any project config, the commands fail unless you pass
`--include-global`.

`[env]` is the exception: it is read from every loaded config, including your
global config. See [Environment variables in the image](#environment-variables-in-the-image).

### Layers {#how-layering-works}

`mise oci build` produces these layers, in order:

1. The base image's layers, copied from the registry unchanged so the registry
   can deduplicate them.
2. The running mise binary at `/usr/local/bin/mise`, unless you pass `--no-mise`.
3. One layer for the apt or apk [system packages](#bootstrap-and-dotfiles-in-oci-images),
   if any.
4. One layer per [vfox plugin](#vfox-plugins) that installed a tool, at
   `/mise/plugins/<name>/`, annotated with `dev.mise.plugin`.
5. One layer per tool at `/mise/installs/<tool>/<version>/`, annotated with
   `dev.mise.tool.short` and `dev.mise.tool.version`.
6. One layer per [copied file or directory](#copy-files-into-the-image),
   annotated with `dev.mise.copy`.
7. One layer for the [dotfiles](#bootstrap-and-dotfiles-in-oci-images), if any.
8. `/etc/mise/config.toml`, which pins each packaged tool to its exact version.
   The image's `MISE_DATA_DIR` and `MISE_CONFIG_DIR=/etc/mise` environment
   variables point the embedded mise at these files.

The paths above use the default mount point, `/mise`; change it with
`--mount-point` or `[oci].mount_point`.

Changing the Node.js version rebuilds only Node.js's layer. The other tool
layers are reused, while the config layer, image config, and manifest change to
record the new version. A tool layer is also rebuilt when its in-image path,
file owner, or contents change, and a Python change can rebuild the layers of
`pypi:` tools that link to that Python.

## Configure the image {#oci-section-in-mise-toml}

Set image options in an `[oci]` section of the project's `mise.toml`:

```toml
[oci]
from = "debian:bookworm-slim"  # base image
tag = "ghcr.io/me/devenv:v1"   # tag recorded in the layout
workdir = "/workspace"         # WORKDIR
entrypoint = []                # ENTRYPOINT
cmd = []                       # CMD
user = "1000:1000"             # USER
user_id = 1000                 # owner UID of files in generated layers
group_id = 1000                # owner GID (defaults to user_id)
mount_point = "/mise"          # where tools live in the image

[oci.labels]
"org.opencontainers.image.source" = "https://github.com/me/my-app"
```

`entrypoint`, `cmd`, and `user` default to the base image's values, and
`workdir` defaults to `/workspace` when neither the base image nor `[oci]` sets
one. `user` sets the image's `USER`; it does not create an account, home
directory, or writable workspace, so use a numeric UID and GID or a user the
base image already has. `user_id` and `group_id` set the owner of files in the
generated layers (`0:0` when unset); `--owner UID[:GID]` overrides them.

### Precedence

Command-line flags override `[oci]`, and `[oci]` overrides the
[`oci.default_from`](/configuration/settings.html#oci.default_from) and
[`oci.default_mount_point`](/configuration/settings.html#oci.default_mount_point)
settings. When several project config files set `[oci]`, for example a monorepo
root and a subproject, mise merges them field by field, and the more specific
file wins each field; `[oci.env]` and `[oci.labels]` merge key by key. `[oci]` in
your global config applies only with `--include-global`.

### Environment variables in the image

The image's environment is built in this order, and later entries win:

1. The base image's environment.
2. `[env]` from every loaded config, including your global
   `~/.config/mise/config.toml`, even without `--include-global`. Templates are
   expanded and `.env` files are read.
3. Variables each tool sets, such as `JAVA_HOME`, `GOROOT`, or `GEM_HOME`, with
   host paths rewritten to the image paths.
4. `[oci.env]`.
5. `PATH`: each tool's bin directories in the image, followed by the `PATH` from
   the entries above.
6. `MISE_DATA_DIR` (the mount point) and `MISE_CONFIG_DIR=/etc/mise`, which
   nothing above can override.

::: warning `[env]` values are baked into the image
Everything in `[env]`, including values loaded from `.env` files and from your
global config, is written into the image config, where anyone with the image can
read it with `docker inspect` or `skopeo inspect`. Move personal tokens out of
your global `[env]`, or unset them, before building an image you will push. Pass
secrets at runtime with `docker run -e`, secret mounts, or your orchestrator, and
put only values that are safe to publish in `[oci.env]`. mise warns with the
number of `[env]` variables it baked in.
:::

A variable that `[env]` marks `required` is satisfied by `[oci.env]` during
`mise oci` commands, so the build host does not need it set. This lets you give
the image a placeholder for a secret that exists only at runtime:

```toml
[env]
AWS_ACCESS_KEY_ID = { required = "AWS access key ID for S3", redact = true }

[oci.env]
AWS_ACCESS_KEY_ID = "placeholder"
```

Only `[oci.env]` in project configs counts, and other commands still enforce
`required`.

### Copy files into the image

Each `[[oci.copy]]` entry, or `--copy HOST:IMAGE` flag, adds one layer after the
tool layers:

```toml
[[oci.copy]]
host = "dist/my-app"
image = "/usr/local/bin/my-app"

[[oci.copy]]
host = "assets"
image = "/srv/app/assets"
```

The source can be a file, a directory, or a symlink, and it must exist when you
build. A directory's contents land at `image`; the source directory's name is
not added. Image paths must be absolute and cannot contain `.` or `..`
components. mise creates parent directories, keeps executable bits, and sets
ownership from `--owner` or `user_id` and `group_id`. A relative `host` path in
`[[oci.copy]]` resolves from the directory of the config file that declares it,
and a relative `--copy` path resolves from the current directory. When layered
configs copy to the same image path, less specific entries come first, so the
most specific config wins, and `--copy` layers come last. mise warns for each
copy, as a reminder to check its contents for secrets.

## System packages and dotfiles {#bootstrap-and-dotfiles-in-oci-images}

`mise oci build` applies the project's `[bootstrap.packages]` and `[dotfiles]`
entries to the image, the image equivalent of the package and dotfile parts of
[`mise bootstrap`](/bootstrap.html). With `--include-global`, entries from the
global and system configs are applied too.

```toml
[bootstrap.packages]
"apt:curl" = "latest"

[dotfiles]
"/etc/profile.d/project.sh" = { source = "profile.sh", mode = "copy" }
"~/.config/app/config.toml" = { source = "config.toml", mode = "template" }
```

Packages can be `apt:` entries on a Debian or Ubuntu base image, or `apk:`
entries on an Alpine or Wolfi base image, but not both in one build. mise
unpacks the base image into a temporary root filesystem, runs the matching
package manager on the host to install into it, and emits the changes as one
layer annotated with `dev.mise.system.packages`. The host needs `apt-get` and
`dpkg` for apt, or `apk` for apk. Apk package scripts run inside a chroot, so
apk layers need a Linux host running mise as root. mise removes package-manager
caches and logs before creating the layer.

Entries for any other package manager, such as `brew:`, make the build fail.
Keep them in a config the image build does not include, or exclude that manager
for the build with the
[`system_packages.managers`](/configuration/settings.html#system_packages.managers)
setting, for example `MISE_SYSTEM_PACKAGES_MANAGERS=apt mise oci build`.

Dotfiles are written as files in the image:

- `symlink` and `symlink-each` entries are copied as file contents, because a
  link to the build host's checkout would be broken in the container.
- Targets that start with `~/` are written under `/root/`.
- An `absent` entry, or a `remove_empty` template that renders empty, adds an
  OCI whiteout, so a file the base image has at that path is hidden.
- Templates that call `secret()` fail the build. Templates render without the
  `env` context, and calling `get_env()`, `exec()`, or `read_file()` fails the
  build, because those values could be recovered from an image layer.
- Tracked files and permissions-only entries are left out.

`mise oci build` fails if an included config sets `[bootstrap.macos.*]`
defaults; keep them in a config the build does not read, such as your global
config without `--include-global`. Other `[bootstrap]` sections, such as
`[bootstrap.files]`, and the `bootstrap` task are not applied to images. Put
container startup work in the image's `entrypoint` or `cmd`.

## Run commands in the image {#mise-oci-run}

`mise oci run` builds the image, or uses the layout you pass with `--image-dir`,
loads it into a container engine, and runs a command with your terminal's
stdin, stdout, and stderr:

```sh
mise oci run -it -- bash
mise oci run -e DEBUG=1 --volume "$PWD:/work" -w /work -- npm test
```

mise prefers Podman, which loads OCI layouts natively, and otherwise streams the
image into Docker with `docker load`; choose one with `--engine`. There is no
`-v` short flag for `--volume`, because `-v` is mise's `--verbose`. When the
command exits, mise removes the container and the loaded image. Pass `--keep` to
keep the image in the engine's storage, tagged `mise-oci:run-*` in Docker.

## Push to a registry {#mise-oci-push}

`mise oci push` builds the image, or takes the layout you pass with
`--image-dir`, and uploads it with mise's own registry client. It does not need
Docker, skopeo, or crane.

```sh
# Build and push in one step
mise oci push ghcr.io/me/devenv:latest

# Push an image built earlier
mise oci build -o ./img
mise oci push --image-dir ./img ghcr.io/me/devenv:v1
```

Include the registry host in the reference. A reference with a path but no host,
such as `me/devenv:latest`, is pushed to Docker Hub (`docker.io`), and a bare
name such as `devenv:latest` is rejected.

mise uploads only the blobs the registry does not have yet, so pushing a mostly
unchanged image transfers little. When the base image is on the destination
registry, its layers are mounted across repositories instead of uploaded. When
it is in the same repository as the destination, mise does not download its
layers either, unless the build installs `[bootstrap.packages]`. Large layers
upload in chunks, and failed requests are retried up to
[`http_retries`](/configuration/settings.html#http_retries) times.

### Authentication {#push-authentication}

mise reads registry credentials from the same files Docker and Podman use, in
this order:

1. `$REGISTRY_AUTH_FILE`
2. `$XDG_RUNTIME_DIR/containers/auth.json`
3. `~/.config/containers/auth.json`
4. `~/.docker/config.json`, or `$DOCKER_CONFIG/config.json`

Inline `auths` entries and credential helpers (`credsStore` and `credHelpers`,
such as `docker-credential-osxkeychain` or `docker-credential-ecr-login`) both
work, so `docker login ghcr.io` or `podman login ghcr.io` is all the setup you
need. Without credentials, mise warns and pushes anonymously, which suits a
local registry. For ghcr.io, the token needs the `write:packages` scope.

### Plain-HTTP registries

mise contacts loopback registries such as `localhost:5000` over plain HTTP, as
Docker does. List any other plain-HTTP registry in
[`oci.insecure_registries`](/configuration/settings.html#oci.insecure_registries):

```toml
[settings.oci]
insecure_registries = ["registry.lan:5000"]
```

### Reuse layers from the registry

`mise oci push` reuses tool layers from the image already at the destination. A
layer is reused when its tool, version, mount point, file owner, and, for vfox
tools, plugin contents all match. Reused tools are not packaged again and do not
need to be installed locally, so a CI push installs and packages only the tools
that changed.

When every push gets a unique tag, reuse layers from another tag in the same
repository with `--cache-from`:

```sh
mise oci push --cache-from ghcr.io/me/dev:latest "ghcr.io/me/dev:$GIT_SHA"
```

`--no-cache` turns off remote and local reuse and rebuilds every tool layer from
local installs; use it if you do not want to trust that a registry layer matches
its annotations. `--cache-from` cannot be combined with `--no-cache` or
`--image-dir`.

Tool environment variables such as `JAVA_HOME` are computed from local installs.
Most backends compute them correctly for a reused tool that is not installed,
but a vfox plugin whose environment hook inspects the install directory can
contribute incomplete values. If the image's environment looks wrong, install
the tool and push with `--no-cache`.

## Multi-arch images

One host builds one platform. To publish a multi-arch tag, run
`mise oci push --update-index` once per architecture: each push uploads its
platform's manifest by digest and points the tag at an OCI image index that
keeps the entries other platforms pushed. Pushing the same platform again
replaces its entry, and a single-platform tag becomes an index without losing
its platform. Layer reuse works through indexes, using the entry for the build
platform.

This GitHub Actions workflow builds one architecture at a time. It assumes the
project has a `mise.toml` and that the workflow can write to the GHCR package:

```yaml
name: Publish development image
on: workflow_dispatch
permissions:
  contents: read
  packages: write
concurrency:
  group: mise-development-image
  cancel-in-progress: false
jobs:
  publish:
    strategy:
      max-parallel: 1
      matrix:
        runner: [ubuntu-24.04, ubuntu-24.04-arm]
    runs-on: ${{ matrix.runner }}
    env:
      MISE_EXPERIMENTAL: "1"
    steps:
      - uses: actions/checkout@v7
      - uses: jdx/mise-action@v5
      - name: Authenticate to GHCR
        env:
          GHCR_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: printf '%s' "$GHCR_TOKEN" | docker login ghcr.io -u "$GITHUB_ACTOR" --password-stdin
      - name: Publish this architecture
        run: mise oci push --update-index "ghcr.io/${GITHUB_REPOSITORY,,}/dev:latest"
```

Updating the index is a read-modify-write, because the registry API has no
conditional writes, so two pushes to the same tag at once can lose an entry.
`max-parallel: 1` runs the architectures one after another, and the workflow's
`concurrency` group keeps two runs from updating the tag at the same time.
Choose runner labels your repository has.

## Layer cache and reproducibility {#layer-reuse}

`mise oci build`, `run`, and `push` share a local cache of packaged tool layers,
so an unchanged tool is compressed once even across separate images and output
directories. The cache key covers each file's contents and image path,
executable permissions, symlink targets, ownership, and relocated contents, but
not the base image. Editing or reinstalling a tool invalidates its entry when
the packaged content changes, even if the version, file size, and modification
time stay the same. A cache hit still reads and hashes the installation; it
skips building and compressing the tar.

`mise oci build --no-cache` and `mise oci push --no-cache` bypass the local
cache. `mise oci run` has no such flag; build with `--no-cache` and pass the
result to `mise oci run --image-dir`. `mise cache clear TOOL` removes one
tool's cached layers, and `mise cache clear` removes all of them. The entries
live in the mise cache directory, so a CI job can keep them by caching
`MISE_CACHE_DIR`.

### Reproducibility

On one host, rebuilding with unchanged inputs produces byte-identical tool
layers. Across machines, layer digests can differ, because compiled files such
as Python bytecode or node-gyp output can embed absolute paths. To make the
image config's timestamps reproducible, set `SOURCE_DATE_EPOCH`:

```sh
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) mise oci build
```

## Supported backends

`mise oci build` packages tools from every backend except asdf, including
vfox plugins. It copies each tool's install directory into its layer and rewrites
executable paths and shebangs that point at the host install directory. That
does not make a tool self-contained: it can still need system libraries, other
runtimes, or files outside its install directory. Declare the runtimes a tool
needs alongside it, and test the image with the commands your project runs.

[asdf plugins](/dev-tools/backends/asdf.html) are rejected. Their install
scripts can write outside the version's directory, which a per-tool layer cannot
capture, and their `exec-env` scripts expect bash at runtime.

### vfox plugins

Tools installed by [vfox plugins](/dev-tools/backends/vfox.html), including
custom [backend plugins](/backend-plugin-development.html) (`my-plugin:tool`),
are packaged like any other tool. mise also copies each plugin, without `.git`,
into its own layer at `/mise/plugins/<name>/`, so the embedded mise can use those
tools without cloning the plugin, and logs each plugin directory it copies.
Plugins built into the mise binary are not copied. A symlink inside a plugin
that resolves outside the plugin directory fails the build instead of copying a
host file into the image; replace it with a copy. Links that are already broken
on the build host are kept as they are.

The plugin's environment hook (`EnvKeys` or `BackendExecEnv`) runs on the build
host. Paths under the host install directory are rewritten to the image path,
and other values are written as they are. mise warns when a value points under
the build host's home directory, which usually does not exist in the container;
override such variables with [`[oci.env]`](#oci-section-in-mise-toml).

A plugin's `PostInstall` or `BackendInstall` hook can run any command, but only
files written to the tool's install directory end up in the image. A tool layer
is reused from the registry only when the plugin's contents match the plugin
that built it, so updating a plugin rebuilds its tools' layers.

## Base images {#registry-base-image-support}

mise pulls base images from any OCI Distribution v2 registry, such as Docker
Hub, ghcr.io, quay.io, or a self-hosted one. It handles anonymous tokens for
public images and uses your `docker login` or `podman login` credentials, so
private base images work too. Pass `scratch` to `--from` to build without a base
image.

A digest pins the base image; a mutable tag can resolve to a new base on a later
build:

```sh
mise oci build --from "REGISTRY/IMAGE@sha256:FULL_DIGEST"
```

Replace the placeholders with an image reference and its complete SHA256
digest.

Choose a base whose libc and shared libraries suit the packaged binaries. The
default, `debian:bookworm-slim`, uses glibc. An Alpine or other musl base needs
musl-compatible or static binaries; changing `--from` does not rebuild the
installed tools for another libc. System libraries a tool needs at runtime must
be in the image.

For every flag, see [`mise oci build`](/cli/oci/build.html),
[`mise oci run`](/cli/oci/run.html), and [`mise oci push`](/cli/oci/push.html).
The [OCI image spec](https://github.com/opencontainers/image-spec) and
[distribution spec](https://github.com/opencontainers/distribution-spec) define
the formats.
