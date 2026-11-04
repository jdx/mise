---
description: "Choose a tool's backend, write backend identifiers, and control which backend a short name uses."
---

# Backends

A backend is the source mise installs a tool from. It lists the tool's
versions, downloads or builds the version you ask for, and tells mise where the
executables are. A registry short name such as `ripgrep` picks a backend for
you; write an identifier such as `github:BurntSushi/ripgrep` to pick one
yourself.

## Backend identifiers {#identifiers}

<span id="what-are-backends"></span>
<span id="backend-specific-settings"></span>

An identifier is a backend name, a colon, and the tool's name within that
backend. A version request can follow `@`:

```sh
mise ls-remote github:BurntSushi/ripgrep
mise use github:BurntSushi/ripgrep@15
mise exec -- rg --version
```

In `github:BurntSushi/ripgrep@15`, `github` is the backend,
`BurntSushi/ripgrep` is the project, and `15` is the version request. `mise use`
installs ripgrep from its GitHub releases and writes the identifier to
`mise.toml`:

```toml
[tools]
"github:BurntSushi/ripgrep" = "15"
```

The part after the colon is whatever the backend uses to find the tool: an
`owner/repo` for `github:`, a package name for `npm:`, a module path for `go:`.
Each backend's page lists its forms. Tool options can follow the name in
brackets, such as `'pypi:black[extras=jupyter]'`. Options belong to a backend:
one that `pypi:` understands means nothing to `aqua:`, so check the backend's
page before you add one.

Any identifier works in `mise.toml` without a registry entry; the
[registry](/registry.html) only adds short names. Run `mise backends ls` to list
the backends your mise supports. Tools with built-in support, such as Node.js
and Python, use the `core` backend (`core:node`); see
[core tools](/core-tools.html).

Versions are not necessarily semantic versions. Each backend decides what
`latest` and a prefix such as `20` mean; see
[version requests](/dev-tools/versions.html).

## Which backend to use {#which-backend-to-use}

<span id="choose-an-installation-source"></span>
<span id="backend-types"></span>
<span id="when-to-use-each-backend"></span>
<span id="backend-capabilities-comparison"></span>

Start with the tool's registry short name. When you choose a backend yourself,
prefer one that installs the publisher's own prebuilt release:

1. `packslip:` when the publisher signs its releases with packslip manifests.
   mise checks the signer and every artifact digest.
2. `aqua:` when the aqua registry has an entry for the tool, or `github:`,
   `gitlab:` or `forgejo:` for release assets on those forges.
3. `http:` or `s3:` for archives you host yourself.
4. A language package backend when the tool ships only as a package, or when
   you need package options such as Cargo features or Python extras. You then
   supply that language's runtime.
5. A plugin only when installing the tool needs custom logic.

| Backend                                                                                                                           | Installs                                      | Example                            | Needs                                            |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ---------------------------------- | ------------------------------------------------ |
| [packslip](/dev-tools/backends/packslip.html)                                                                                     | Releases with signed manifests                | `packslip:github.com/jdx/hk`       | A publisher that ships packslip manifests        |
| [aqua](/dev-tools/backends/aqua.html)                                                                                             | Release assets described by the aqua registry | `aqua:BurntSushi/ripgrep`          | An aqua registry entry for the tool              |
| [github](/dev-tools/backends/github.html), [gitlab](/dev-tools/backends/gitlab.html), [forgejo](/dev-tools/backends/forgejo.html) | Release assets from a forge                   | `github:BurntSushi/ripgrep`        | A release asset for your OS and architecture     |
| [http](/dev-tools/backends/http.html), [s3](/dev-tools/backends/s3.html)                                                          | Archives at URLs you provide                  | `http:my-tool` with a `url` option | Download URLs; S3 also needs AWS credentials     |
| [cargo](/dev-tools/backends/cargo.html)                                                                                           | Rust crates                                   | `cargo:eza`                        | Rust; cargo-binstall for prebuilt binaries       |
| [go](/dev-tools/backends/go.html)                                                                                                 | Go modules                                    | `go:github.com/DarthSim/hivemind`  | Go                                               |
| [npm](/dev-tools/backends/npm.html)                                                                                               | npm packages                                  | `npm:prettier`                     | Node.js to run most packages                     |
| [pypi](/dev-tools/backends/pypi.html) (also `pipx:`)                                                                              | Python packages                               | `pypi:black`                       | uv or pipx, and Python                           |
| [gem](/dev-tools/backends/gem.html)                                                                                               | Ruby gems                                     | `gem:rubocop`                      | Ruby                                             |
| [dotnet](/dev-tools/backends/dotnet.html)                                                                                         | .NET tools from NuGet                         | `dotnet:GitVersion.Tool`           | The .NET SDK and the tool's runtime              |
| [spm](/dev-tools/backends/spm.html)                                                                                               | Swift package executables                     | `spm:nicklockwood/SwiftFormat`     | Swift                                            |
| [conda](/dev-tools/backends/conda.html)                                                                                           | Conda packages and their dependencies         | `conda:ffmpeg`                     | Nothing else                                     |
| [spinel](/dev-tools/backends/spinel.html) (experimental)                                                                          | Ruby programs compiled to native executables  | `spinel:tobi/try`                  | `experimental = true`, Spinel, Git, a C compiler |
| [vfox](/dev-tools/backends/vfox.html)                                                                                             | Tools installed by a Lua plugin               | `vfox:version-fox/vfox-cmake`      | Trust in the plugin's code                       |
| [asdf](/dev-tools/backends/asdf.html) (legacy)                                                                                    | Tools installed by a shell-script plugin      | `asdf:MetricMike/asdf-awscli`      | Bash and Unix tools; not available on Windows    |
| [ubi](/dev-tools/backends/ubi.html) (deprecated)                                                                                  | GitHub and GitLab release assets              | `ubi:BurntSushi/ripgrep`           | Migrate to `github:`                             |
| core                                                                                                                              | Tools built into mise                         | `core:node`                        | See [core tools](/core-tools.html)               |

Language package backends (cargo, go, npm, pypi, gem, dotnet) install
command-line tools, each into its own directory. They do not manage your
project's dependencies: keep those in `package.json`, `Cargo.toml`, `go.mod`,
`Gemfile` or `pyproject.toml`, or see
[project dependencies](/dev-tools/deps.html).

Verification differs by backend and by tool. A backend can check a signature or
provenance only when the publisher provides one, so read
[security](/security.html) before you rely on a particular check.

`ubi:` is deprecated: mise warns when it uses it, and mise 2027.1.0 removes it.
Use the `github:` backend (or `gitlab:` or `http:` for other hosts); see the
[ubi migration guide](/dev-tools/backends/ubi.html).

[Backend plugins](/backend-plugin-development.html) add a backend of your own
that manages a family of tools under its own prefix. The registry has stricter
rules for which backends a new short name can use; see
[backend acceptance tiers](/contributing/registry.html#backend-acceptance-tiers).

## How mise picks a backend {#how-backend-selection-works}

An identifier with a backend prefix, such as `aqua:BurntSushi/ripgrep`, always
uses that backend. For a short name such as `ripgrep`, mise uses the first of
these that applies:

1. A [`MISE_BACKENDS_<TOOL>`](#environment-variable-overrides) environment
   variable.
2. A [`[tool_alias]`](/dev-tools/aliases.html) entry that names a backend, such
   as `node = "core:node"`.
3. A plugin URL for the tool under `[plugins]` in a config file.
4. The backend recorded for the tool in [`mise.lock`](/dev-tools/mise-lock.html),
   when that backend can serve the requested version.
5. An installed plugin with the same name, unless its backend is disabled.
6. The first backend in the tool's registry entry that runs on this platform,
   is not listed in [`disable_backends`](#disable-backends), and supports the
   requested version. Experimental backends count only with
   [`experimental = true`](/configuration/settings.html#experimental).

`mise tool` shows the result, and `mise registry` shows the registry's list in
order:

```sh
mise tool ripgrep --backend
# aqua:BurntSushi/ripgrep
mise registry ripgrep
# aqua:BurntSushi/ripgrep asdf:https://gitlab.com/wt0f/asdf-ripgrep cargo:ripgrep
```

When the registry moves a tool to another backend, a lockfile keeps the tool on
the backend it recorded, and mise warns that the registry has a newer one.
[`mise backends switch`](/cli/backends/switch.html) moves those lock entries to
the registry's current backend and reinstalls the affected versions.

### Version-specific backends {#version-specific-backends}

A registry entry can limit a backend to part of a tool's version range. hk's
entry starts packslip at 1.58.1, so `mise use hk@1.58.0` installs from aqua and
`mise use hk@1.58.1` installs from packslip. A prefix entirely below the
boundary, such as `hk@1.57`, also uses aqua. `latest` and prefixes that span the
boundary, such as `hk@1.58`, keep the normal order.

A backend can also set an exclusive `max_version`, so that requests below it use
that backend and newer ones use the next backend in the list. These
boundaries apply only to short names: an explicit identifier ignores them, and a
matching lockfile entry keeps its recorded backend. For the registry format,
see [minimum backend versions](/contributing/registry.html#minimum-backend-versions).

## Choose a backend yourself {#choose-a-backend-yourself}

<span id="configuration-and-overrides"></span>
<span id="force-backend-for-tool"></span>
<span id="registry-system"></span>

To use a particular source in one project, write its identifier as the key:

```toml [mise.toml]
[tools]
"aqua:BurntSushi/ripgrep" = "15"
```

To keep the short name in `[tools]` and change only its backend, add a
[tool alias](/dev-tools/aliases.html). Teammates get the same backend because
the alias lives in the same file:

```toml [mise.toml]
[tool_alias]
node = "core:node"

[tools]
node = "24"
```

### Environment variable override {#environment-variable-overrides}

`MISE_BACKENDS_<TOOL>` overrides the backend for one short name, ahead of
aliases, the lockfile and the registry. Use it for a single command or CI job:

```sh
# install ripgrep from GitHub releases instead of aqua
MISE_BACKENDS_RIPGREP='github:BurntSushi/ripgrep' mise install ripgrep@15
```

Write the tool name in uppercase with `-` replaced by `_`: `my-tool` becomes
`MISE_BACKENDS_MY_TOOL`. An exported override applies to every later command in
that shell, so check your environment when two machines resolve the same short
name differently.

### Disable backends {#disable-backends}

Set [`disable_backends`](/configuration/settings.html#disable_backends) to stop
mise from resolving or installing tools through those backends. To apply it
everywhere, put it in your global config:

```toml [~/.config/mise/config.toml]
[settings]
disable_backends = ["asdf", "vfox"]
```

`mise settings disable_backends=asdf,vfox` writes the same setting to your
global config. It replaces the whole list, so name every backend you want
disabled.

Short names then skip those backends in their registry list, and installing an
explicit identifier such as `asdf:owner/asdf-tool` fails. Versions already
installed stay on disk and work again if you enable the backend. mise never
uses the asdf backend on Windows, whatever this setting says.

## Troubleshooting {#troubleshooting}

<span id="verify-the-result"></span>
<span id="troubleshooting-backend-issues"></span>
<span id="debug-backend-selection"></span>

These commands show why a short name resolved the way it did:

```sh
mise tool node --backend   # the backend mise uses
mise registry node         # the registry's backends, in order
mise plugins ls            # installed plugins, which can replace the registry choice
mise config ls             # config files that apply in this directory
mise ls --current          # selected versions and where each was set
```

A version listed by `mise ls-remote` can still lack a build for your OS and
architecture. After `mise use`, run the tool with
`mise exec -- <command> --version`. If mise cannot find a matching asset, the
backend's page lists its platform and asset options. If selection is right but
installation fails, check the backend's requirements and authentication.
`MISE_DEBUG=1 mise install <tool>` adds diagnostic output; remove credentials
from it before you share it.

To make installs reproducible, record resolved versions and checksums in
[`mise.lock`](/dev-tools/mise-lock.html).
