# mise oci <Badge type="warning" text="experimental" />

`mise oci build` turns a `mise.toml` into a container image, with one
[OCI](https://github.com/opencontainers/image-spec) layer per installed tool.

Tool layers can be reused independently when a version changes. Image config,
manifests, and layers whose inputs changed still need updating; a Python change
can also invalidate dependent pipx layers.

Build on a **Linux host with the target architecture**. mise packages the installed
host binaries; it does not cross-compile or download another OS's tools for the
image. `mise oci run` also requires Docker or Podman. Building and pushing an OCI
layout use mise's own image and registry support.

::: warning Experimental
`mise oci build` is experimental. Enable it with:

```sh
mise settings set experimental=true
# or, per-invocation:
MISE_EXPERIMENTAL=1 mise oci build …
```

Flags, output layout, and defaults may change in future releases.
:::

## Commands at a glance

| Command          | What it does                                                             |
| ---------------- | ------------------------------------------------------------------------ |
| `mise oci build` | Produce an OCI image layout on disk.                                     |
| `mise oci run`   | Build (or reuse) an image and run a command inside it via podman/docker. |
| `mise oci push`  | Build (or reuse) an image and push it to a registry.                     |

## Quick start

On Linux, start with a project configuration such as:

```toml [mise.toml]
[settings]
experimental = true

[tools]
node = "24"
```

Build a local image layout, then verify its executable through a container engine:

```sh
mise oci build -o ./mise-oci
mise oci run --image-dir ./mise-oci -- node --version
```

The default base is `debian:bookworm-slim`. Add the generated `mise-oci/` directory
to `.gitignore`. This creates a tool environment; it does not automatically copy
your application or install its package dependencies. Use a volume for development
or [`oci.copy`](/dev-tools/mise-oci.html#oci-section-in-mise-toml) for files that belong in the image.

To inspect the layout with an external tool, install `skopeo` and run
`skopeo inspect oci:./mise-oci`. To publish it, follow [push authentication](#push-authentication)
and choose a registry/repository you can write to:

```sh
mise oci push --image-dir ./mise-oci ghcr.io/OWNER/IMAGE:TAG
```

Replace the uppercase placeholders. This command publishes the image to that
registry; it is separate from the local build and run checks.

## How layering works

Given this `mise.toml`:

```toml
[tools]
node = "20"
python = "3.12"
jq = "1.8.1"
```

`mise oci build` produces layers roughly like this:

1. **Base image layers** (e.g. `debian:bookworm-slim`) — copied through from
   the registry unchanged, so registry dedup kicks in.
2. **mise binary** at `/usr/local/bin/mise` (skip with `--no-mise`).
3. **Configured apt or apk `[bootstrap.packages]`**, if any, installed into the
   base rootfs and emitted as one package layer.
4. **One layer per tool**, each rooted at
   `/mise/installs/<plugin>/<version>/`. Annotated with
   `dev.mise.tool.short` and `dev.mise.tool.version`.
5. **Configured `[dotfiles]`**, if any, baked as image files.
6. **Synthesized `/etc/mise/config.toml`** referencing `/mise` as the data
   directory.

Changing Node.js leaves unrelated tool archives reusable. The generated
configuration and image manifest still reflect the new version. Reuse also
depends on the in-image path, file ownership, and any relocation inputs.

## `mise oci build`

```sh
mise oci build [-o PATH] [--from REF] [--tag REF] [--mount-point PATH]
               [--copy HOST_PATH:IMAGE_PATH]...
               [--no-mise] [--owner UID[:GID]]
```

- `-o, --output PATH` — output directory (default `./mise-oci`)
- `--from REF` — base image reference (overrides `[oci].from` and the
  `oci.default_from` setting). Use `scratch` to build without a base.
- `-t, --tag REF` — tag written to `index.json` as the
  `org.opencontainers.image.ref.name` annotation
- `--mount-point PATH` — where mise installs live inside the image
  (default `/mise`). Must be absolute.
- `--copy HOST_PATH:IMAGE_PATH` — copy a host file or directory to an
  absolute path in the image. Repeat the flag for multiple payloads. Each
  payload is emitted as an independent, content-addressed layer after the
  tool layers.
- `--no-mise` — don't embed the running mise binary at
  `/usr/local/bin/mise`
- `--owner UID[:GID]` — numeric owner for every generated layer entry.
  Defaults to `[oci].user_id` / `[oci].group_id`, then `0:0`. If GID is
  omitted, it defaults to UID. This affects file ownership only, not the
  image `USER` directive.

## `mise oci run`

Build (or reuse) an image and run a command inside it, like
`docker run` / `podman run`. Stdin/stdout/stderr are inherited.

```sh
mise oci run [--engine ENGINE] [--image-dir DIR]
             [--from REF] [--mount-point PATH] [--no-mise]
             [--owner UID[:GID]]
             [-i] [-t] [-e KEY=VAL]... [--volume HOST:CONTAINER]...
             [-w DIR] [--keep]
             -- <cmd> [args...]
```

- `--engine` — `auto` (default, prefers podman), `podman`, or `docker`.
- `--image-dir` — skip the build and use an existing OCI layout.
- `--owner UID[:GID]` — numeric owner for generated layer entries when
  building fresh; it cannot be combined with `--image-dir`.
- `-i`, `-t`, `-e`, `--volume`, `-w`, `--keep` — pass through to the
  underlying engine the same way `docker run` uses them. (There's no
  `-v` short flag for `--volume` because mise reserves `-v` for
  `--verbose`; use `--volume` or `--mount`.)

Examples:

```sh
# Interactive shell
mise oci run -it -- bash

# One-shot command with env + volume
mise oci run -e DEBUG=1 --volume "$PWD:/work" -w /work -- npm test

# Re-use a previously built layout
mise oci build -o ./img
mise oci run --image-dir ./img -- node --version
```

**Requirements:** either `podman` (native OCI-layout support) or
`docker` (mise streams the image into the daemon via `docker load`).

## `mise oci push`

Build (or reuse) an image and push it to a registry with mise's
built-in registry client — no skopeo, crane, or docker daemon
required. Only blobs the registry doesn't already have are uploaded,
so repeat pushes of a mostly-unchanged toolset transfer very little.
When the base image lives on the destination registry, its blobs are
cross-repository mounted instead of re-uploaded (no bytes transferred).
Large layers upload in chunks with progress bars, and transient network
failures are retried with backoff (`http_retries` controls attempts).

### Layer reuse

Tool layers whose cache key (tool, version, in-image prefix, and file
owner) matches the previously pushed image are **reused from the
registry instead of rebuilt** — skipping the tar/gzip work entirely.
Reused tools don't even need to be installed locally, which makes CI
pushes fast: only tools whose version actually changed get installed
and packaged.

- By default the cache is the destination ref itself (the image
  previously pushed under that tag).
- `--cache-from REF` reuses layers from another tag in the **same
  repository** — useful when every push gets a unique tag:

  ```sh
  mise oci push --cache-from ghcr.io/me/dev:latest ghcr.io/me/dev:$GIT_SHA
  ```

- `--no-cache` disables reuse and rebuilds every layer from the local
  installs (docker-style escape hatch — reuse trusts that the
  registry's layer content matches its annotations, rather than
  rebuilding the exact bytes locally).

One caveat: environment derivation (`JAVA_HOME`-style `exec_env` vars)
runs against local installs. For a reused tool that isn't installed,
most backends still derive paths correctly, but exotic backends may
contribute incomplete env — pass `--no-cache` (with the tool installed)
if the image config looks wrong.

```sh
mise oci push [--image-dir DIR]
              [--from REF] [--mount-point PATH] [--no-mise]
              [--owner UID[:GID]]
              <REGISTRY_REF>
```

- `<REGISTRY_REF>` — fully-qualified destination (e.g.
  `ghcr.io/me/devenv:latest`). Must include a registry host. Loopback
  registries (`localhost:5000/…`) are contacted over plain HTTP, the
  same insecure-by-default convention docker applies. Non-loopback
  plain-HTTP registries (a homelab `registry.lan:5000`) must be opted
  in via the `oci.insecure_registries` setting:

  ```toml
  [settings.oci]
  insecure_registries = ["registry.lan:5000"]
  ```

- `--image-dir` — push an existing OCI layout instead of building.

- `--owner UID[:GID]` — numeric owner for generated layer entries when
  building fresh; it cannot be combined with `--image-dir`.

Examples:

```sh
# Build + push in one shot
mise oci push ghcr.io/me/devenv:latest

# Push an image built earlier
mise oci build -o ./img
mise oci push --image-dir ./img ghcr.io/me/devenv:v1
```

### Push authentication

Credentials are resolved from the same sources docker and podman use,
in this order:

1. `$REGISTRY_AUTH_FILE`
2. `$XDG_RUNTIME_DIR/containers/auth.json` (podman)
3. `~/.config/containers/auth.json`
4. `~/.docker/config.json` (or `$DOCKER_CONFIG/config.json`)

Both inline `auths` entries and credential helpers
(`credsStore` / `credHelpers`, e.g. `docker-credential-osxkeychain`,
`docker-credential-ecr-login`) are supported — so a plain
`docker login ghcr.io` or `podman login ghcr.io` is all the setup
needed. When no credentials are found, mise pushes anonymously (useful
for local registries) and warns.

For ghcr.io, the token needs the `write:packages` scope.

### `[oci]` section in `mise.toml`

```toml
[oci]
from        = "debian:bookworm-slim"  # base image ref
tag         = "ghcr.io/me/devenv:v1"  # default tag for the built image
workdir     = "/workspace"             # WORKDIR
entrypoint  = []           # ENTRYPOINT
cmd         = []                        # CMD
user        = "1000:1000"                # USER
user_id     = 1000                      # tar layer entry UID (file ownership)
group_id    = 1000                      # tar layer entry GID (defaults to user_id)
mount_point = "/mise"                  # where tools install in the image

[[oci.copy]]
host  = "dist/my-app"
image = "/usr/local/bin/my-app"

[[oci.copy]]
host  = "assets"
image = "/srv/app/assets"

# Extra env baked into the image config (image-only — won't shadow MISE_*).
[oci.env]
NODE_ENV = "production"

# Labels baked into the image config.
[oci.labels]
"org.opencontainers.image.source" = "https://github.com/me/my-app"
```

The copy examples require `dist/my-app` and `assets` to exist.
`[oci].user` sets the image `USER` directive; it does not create an account, home
directory, or writable workspace. Use a numeric UID/GID or a user already
provided by the base image. `[oci].user_id` and
`[oci].group_id` set layer file ownership; if no `group_id` is configured,
it defaults to the resolved `user_id`.

CLI flags override the `[oci]` section. The `[oci]` section overrides the
`oci.default_from` / `oci.default_mount_point` settings.

When `mise.toml` files are layered (global + project), sections are merged
field-by-field with the more specific file winning per field.

Copy sources may be files, directories, or symlinks. Directory contents land
at `image`; the source directory name is not added. Image paths must be
absolute and may not contain `.` or `..` components. Parent directories are
created automatically, executable bits are preserved, and ownership follows
`--owner` or `[oci].user_id` / `[oci].group_id`. Copy layers are annotated
with `dev.mise.copy=<image path>` so they can be identified during inspection.
Relative `host` paths in `[[oci.copy]]` resolve from the directory containing
the config file that declares them; relative CLI paths resolve from the current
working directory.
When layered configs copy to the same image path, less-specific entries are
emitted first so the most-specific config wins. CLI copies are emitted last.

### `[bootstrap]` and `[dotfiles]` in OCI images

`mise oci build` applies project-scoped `[bootstrap.packages]` and
`[dotfiles]` entries to the image. This is the OCI equivalent of the
declarative package and dotfile parts of `mise bootstrap`.
Pass `--include-global` to also include `[bootstrap.packages]` and
`[dotfiles]` from global configs.

```toml
[bootstrap.packages]
"apt:curl" = "latest"

[dotfiles]
"/etc/profile.d/project.sh" = { source = "profile.sh", mode = "copy" }
"~/.config/app/config.toml" = { source = "config.toml", mode = "template" }
```

For packages, OCI builds support `apt:` entries with a Debian/Ubuntu base image
and `apk:` entries with an Alpine/Wolfi base image. mise unpacks the base image
into a temporary rootfs, calls the matching host package manager to install into
that rootfs, then emits the filesystem changes as one OCI layer annotated with
`dev.mise.system.packages=apt` or `dev.mise.system.packages=apk`. A build may
use only the package manager matching its base image; mixing `apt:` and `apk:`
entries is rejected.

The host must provide `apt-get` and `dpkg` for apt layers, or `apk` for apk
layers. Apk package scripts execute inside a chroot, so apk layers currently
require a Linux host running mise as root. `--no-cache` is passed to apk and
transient package-manager cache and log files are removed before the layer is
created.

For image builds, `symlink` and `symlink-each` entries are copied as file
content. Host symlinks would usually point back to the checkout path and be
broken inside the container, so the image receives the resolved contents
instead. Targets beginning with `~/` are written under `/root/`.

`[bootstrap.macos.defaults]` and the imperative `bootstrap` task are not run by
`mise oci build`. macOS defaults do not apply to Linux OCI images, and
container-specific startup work belongs in the image entrypoint or command.

### Settings

| Setting                   | Default                | Description                                |
| ------------------------- | ---------------------- | ------------------------------------------ |
| `oci.default_from`        | `debian:bookworm-slim` | Default base image when none is specified. |
| `oci.default_mount_point` | `/mise`                | Where tools install inside the image.      |

Choose a base compatible with the packaged binaries and their shared libraries.
The default uses glibc. An Alpine/musl base requires musl-compatible or suitable
static binaries; changing `--from` does not rebuild the installed tools for a
different libc. System libraries required at runtime must be present in the image.

## Environment variables in the image

The image config's `Env` is built in this order (later entries win):

1. Base image env (from the pulled `--from` image's config).
2. Your `[env]` section from `mise.toml` (fully resolved — templates
   expanded, `.env` files read).
3. Each tool's `exec_env()` — e.g. `JAVA_HOME`, `GOROOT`, `GEM_HOME`.
   Paths are rebased from the host install dir onto the in-image path.
4. `[oci].env` entries.
5. Synthesized PATH (each tool's bin paths in the image) plus the
   inherited PATH.
6. `MISE_DATA_DIR=/mise` and `MISE_CONFIG_DIR=/etc/mise` — always
   applied last so they can't be shadowed.

::: warning Secrets in `[env]` are baked into the image
Anything in your mise `[env]` section — including values loaded from
`.env` files — is written into the image config JSON and visible to
anyone who runs `docker inspect` / `skopeo inspect`. **Do not put
secrets there.** Use `docker run -e`, secret mounts, or orchestrator
secrets at runtime. Use `[oci].env` only for values that are safe to
live in the image.

mise emits a warning with the number of `[env]` vars it baked in.
:::

## Supported backends

The builder accepts built-in backends and packages each selected tool's install
directory. It also relocates supported executable paths and shebangs. Acceptance
by the builder does not guarantee that a tool is self-contained: system libraries,
external runtimes, or paths outside its installation may still be needed.
Declare required runtimes alongside their tools and verify the resulting image
with the commands your project actually runs.

asdf and vfox plugins, including custom vfox backend plugins, are rejected. Their
installation hooks can write outside the per-version directory, which the
per-tool layer model cannot capture reliably.

## Registry base-image support

Base images can be pulled from any OCI Distribution v2 registry —
Docker Hub, ghcr.io, quay.io, self-hosted, etc. Anonymous token auth
is handled automatically for public images; when you're logged in
(`docker login` / `podman login`), those credentials are used, so
private base images work too.

Digest references are supported:

```sh
mise oci build --from "REGISTRY/IMAGE@sha256:FULL_DIGEST"
```

Replace the placeholders with an actual image reference and its complete
SHA256 digest. A digest pins the base image; a mutable tag can resolve to a new
base on a later build.

## Reproducibility

On the same host, re-running `mise oci build` with unchanged inputs
produces byte-identical tool layer digests. Across machines, layer
digests may drift because compiled artifacts (pyc bytecode, generated
node-gyp output, etc.) can embed absolute paths.

For fully-reproducible image config timestamps, set
`SOURCE_DATE_EPOCH`:

```sh
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) mise oci build
```

## Cross-platform builds

OCI images are Linux-targeted. Building on macOS or Windows produces an
image whose `os` field is `linux`, but any embedded binaries (mise and
every tool layer) are still host-native — they will fail with
`Exec format error` when executed inside the container.

Build on a Linux host or in a Linux development container that already has mise
and the required tool-installation dependencies. A stock `debian` image does
not contain mise. Do not mount macOS or Windows tool installations into that
container as substitutes for Linux installations. mise warns when host and image
platforms do not match.

### Multi-arch images

A single host builds a single platform, but `mise oci push
--update-index` lets one runner per architecture assemble a multi-arch
tag: each push uploads its platform manifest by digest and points the
tag at an OCI **image index** that preserves the entries other
platforms pushed.

For example, the following GitHub Actions job builds one architecture at a time.
It assumes the project has `mise.toml`, publishes to GHCR, and grants the workflow
access to that package:

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
      - uses: actions/checkout@v6
      - uses: jdx/mise-action@v2
      - name: Authenticate to GHCR
        env:
          GHCR_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: printf '%s' "$GHCR_TOKEN" | docker login ghcr.io -u "$GITHUB_ACTOR" --password-stdin
      - name: Publish this architecture
        run: mise oci push --update-index "ghcr.io/${GITHUB_REPOSITORY,,}/dev:latest"
```

Choose runner labels available to your repository. Matrix serialization prevents
the two platform pushes from racing, and workflow concurrency prevents overlapping
runs of this workflow from updating the same tag simultaneously.

Re-pushing the same platform replaces its entry (no duplicates), and a
previously single-arch tag is upgraded to an index without losing the
existing platform. Layer reuse works through indexes — the cache
resolves to the entry matching the build platform.

The index update is read-modify-write (the Distribution API has no
conditional writes), so concurrent pushes to the same tag from
different runners can race — sequence them as above.

## Known limitations (v1)

- `asdf` / `vfox` backends are rejected (see above).
- Cross-platform builds produce broken images (binaries are host-native);
  run the build on a Linux host.
- The base image must supply a compatible libc and other runtime libraries.
- `mise oci run` needs a container engine (podman or docker) — mise has
  no built-in container runtime. Pushing needs no external tools.

## See also

- [`mise oci build`](/cli/oci/build.md) — full CLI reference
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
- [OCI Distribution Spec](https://github.com/opencontainers/distribution-spec)
