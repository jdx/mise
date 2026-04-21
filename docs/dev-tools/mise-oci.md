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

## Quick start

```sh
# Build an image from the current mise.toml using the default base
# (debian:bookworm-slim). Output goes to ./mise-oci/.
mise oci build

# Inspect it
skopeo inspect oci:./mise-oci

# Push to a registry
skopeo copy oci:./mise-oci docker://ghcr.io/me/devenv:latest

# Or load straight into a local Docker daemon
skopeo copy oci:./mise-oci docker-daemon:me/devenv:latest
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
4. **Synthesized `/etc/mise/config.toml`** referencing `/mise` as the data
   directory.

Bumping `node` from `20.10` to `20.11` only invalidates the node layer.
Python, jq, mise, the base, and the synthesized config are reused from
the previous build (or from the registry, on pull).

## Configuration

### CLI flags

```sh
mise oci build [-o PATH] [--from REF] [--tag REF] [--mount-point PATH] [--no-mise]
```

- `-o, --output PATH` — output directory (default `./mise-oci`)
- `--from REF` — base image reference (overrides `[oci].from` and the
  `oci.default_from` setting). Use `scratch` to build without a base.
- `-t, --tag REF` — tag written to `index.json` as the
  `org.opencontainers.image.ref.name` annotation
- `--mount-point PATH` — where mise installs live inside the image
  (default `/mise`). Must be absolute.
- `--no-mise` — don't embed the running mise binary at
  `/usr/local/bin/mise`

### `[oci]` section in `mise.toml`

```toml
[oci]
from       = "debian:bookworm-slim"  # base image ref
tag        = "ghcr.io/me/devenv:v1"  # default tag for the built image
workdir    = "/workspace"             # WORKDIR
entrypoint = ["bash", "-l"]           # ENTRYPOINT
cmd        = []                        # CMD
user       = "nonroot"                # USER
mount_point = "/mise"                 # where tools install in the image

# Extra env baked into the image config (image-only — won't shadow MISE_*).
[oci.env]
NODE_ENV = "production"

# Labels baked into the image config.
[oci.labels]
"org.opencontainers.image.source" = "https://github.com/me/my-app"
```

CLI flags override the `[oci]` section. The `[oci]` section overrides the
`oci.default_from` / `oci.default_mount_point` settings.

When `mise.toml` files are layered (global + project), sections are merged
field-by-field with the more specific file winning per field.

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

v1 can pull base images from any OCI Distribution v2 registry that
accepts anonymous pulls:

- Docker Hub (`debian`, `ubuntu`, `node`, …) — token auth handled
  anonymously.
- GitHub Container Registry (`ghcr.io/…`) — public images only.
- Quay.io (`quay.io/…`) — public images only.
- Self-hosted / other registries — work if no auth is required.

Authenticated pulls (private base images) are a follow-up.

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
- Only anonymous registry pulls; no auth.
- Cross-platform builds produce broken images (binaries are host-native).
- No built-in registry push — use `skopeo copy oci:…` or `crane push`.
- Alpine / musl base images will break most tools.

## See also

- [`mise oci build`](/cli/oci/build.md) — full CLI reference
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
- [skopeo](https://github.com/containers/skopeo) for pushing images
