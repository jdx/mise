# mise oci <Badge type="warning" text="experimental" />

`mise oci build` turns a `mise.toml` into a container image, with one
[OCI](https://github.com/opencontainers/image-spec) layer per installed tool.

The payoff is that **bumping any single tool version only invalidates one
content-addressable blob**. With a Dockerfile, each `RUN install_tool` is
stacked on the one before it — changing an early `RUN` invalidates every
later layer. mise's on-disk layout (every tool installed in an isolated
`$MISE_DATA_DIR/installs/<plugin>/<version>/` directory) makes layer ordering
semantically irrelevant, so swapping a tool's version swaps a single layer
and everything else (the base image, other tools, mise itself, image config)
is reused unchanged.

::: warning Experimental
`mise oci build` is experimental. Enable it with:

```sh
mise settings experimental=true
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

```sh
# Build an image from the current mise.toml using the default base
# (debian:bookworm-slim). Output goes to ./mise-oci/.
mise oci build

# Run an interactive shell in the image (uses podman if present, else
# docker).
mise oci run -it -- bash

# Push to a registry with the built-in client (no skopeo/crane needed).
mise oci push ghcr.io/me/devenv:latest

# The output is a standard OCI image layout, so external tools work too:
skopeo inspect oci:./mise-oci
```

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
3. **One layer per tool**, each rooted at
   `/mise/installs/<plugin>/<version>/`. Annotated with
   `dev.mise.tool.short` and `dev.mise.tool.version`.
4. **Configured apt `[bootstrap.packages]`**, if any, installed into the base
   rootfs and emitted as one package layer.
5. **Configured `[dotfiles]`**, if any, baked as image files.
6. **Synthesized `/etc/mise/config.toml`** referencing `/mise` as the data
   directory.

Bumping `node` from `20.10` to `20.11` only invalidates the node layer.
Python, jq, mise, the base, and the synthesized config are reused from
the previous build (or from the registry, on pull).

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
entrypoint  = ["bash", "-l"]           # ENTRYPOINT
cmd         = []                        # CMD
user        = "nonroot"                # USER
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

`[oci].user` sets the image `USER` directive. `[oci].user_id` and
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

For packages, OCI builds currently support `apt:` entries with a Debian/Ubuntu
base image. mise unpacks the base image into a temporary rootfs, calls the
host `apt-get` to install into that rootfs, then emits the filesystem changes
as one OCI layer annotated with `dev.mise.system.packages=apt`. Other system
package managers are rejected for OCI builds for now.

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

The default base is **glibc-based on purpose**. Alpine / musl would break
most mise-installed prebuilt binaries (Node, Python wheels, Ruby gems).
If you know your tools are statically linked you can opt in with
`--from alpine:…` — expect trouble otherwise.

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

mise emits a warning listing the count of `[env]` vars it baked in.
:::

## Supported backends

All of mise's first-party backends install entirely under their
per-version install directory, so they work as per-tool layers:

`core`, `aqua`, `cargo`, `npm`, `go`, `pipx`, `github`, `gitlab`,
`forgejo`, `ubi`, `spm`, `http`, `s3`, `gem`, `conda`, `dotnet`.

**Not supported in v1:** `asdf` and `vfox` plugins (including third-party
vfox plugins). Their install scripts can write outside the per-version
directory, breaking the one-layer-per-tool invariant. Using them errors
out with a clear message.

## Registry base-image support

Base images can be pulled from any OCI Distribution v2 registry —
Docker Hub, ghcr.io, quay.io, self-hosted, etc. Anonymous token auth
is handled automatically for public images; when you're logged in
(`docker login` / `podman login`), those credentials are used, so
private base images work too.

Digest references are supported:

```sh
mise oci build --from ubuntu@sha256:e3b0c44298fc...
```

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

OCI images are linux-targeted. Building on macOS or Windows produces an
image whose `os` field is `linux`, but any embedded binaries (mise and
every tool layer) are still host-native — they will fail with
`Exec format error` when executed inside the container.

For a working image, run `mise oci build` on a linux host (or inside a
linux container — `docker run -v $PWD:/src -w /src debian mise oci build`
works). mise prints a warning when this mismatch is detected.

## Known limitations (v1)

- `asdf` / `vfox` backends are rejected (see above).
- Cross-platform builds produce broken images (binaries are host-native);
  run the build on a linux host.
- Alpine / musl base images will break most tools.
- `mise oci run` needs a container engine (podman or docker) — mise has
  no built-in container runtime. Pushing needs no external tools.

## See also

- [`mise oci build`](/cli/oci/build.md) — full CLI reference
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
- [OCI Distribution Spec](https://github.com/opencontainers/distribution-spec)
