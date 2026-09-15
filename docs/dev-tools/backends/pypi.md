---
description: "Install Python command-line applications in isolated virtual environments."
---

# PyPI Backend

The `pypi` backend installs Python command-line applications in isolated virtual
environments. Each tool gets its own dependencies. Use a project environment
and pip or uv for application libraries such as NumPy and requests.

## Quick start {#dependencies}

Install uv, Python, and a CLI from PyPI:

```sh
mise use python@3.14 uv pypi:black
mise exec -- black --version
```

### Project configuration {#usage}

This adds the following to `mise.toml`:

```toml
[tools]
python = "3.14"
uv = "latest"
"pypi:black" = "latest"
```

Add `-g` to `mise use` to install tools globally.

mise uses uv to install tools. With [dependency locking](#dependency-locking),
it runs `uv sync --frozen`; version-only installs use `uv tool install`.
If uv is unavailable, version-only installs fall back to `pipx install`.
See [Using pipx](#using-pipx) to select that installer explicitly.

## Package sources {#supported-pipx-syntax}

Use `pypi:black` for the PyPI distribution or `pypi:psf/black` for its GitHub
source. Their releases and installation requirements can differ.

| Source                   | Example                                          |
| ------------------------ | ------------------------------------------------ |
| PyPI, latest version     | `pypi:black`                                     |
| PyPI, specific version   | `pypi:black@24.3.0`                              |
| GitHub, default branch   | `pypi:psf/black`                                 |
| GitHub, specific release | `pypi:psf/black@24.3.0`                          |
| Git repository           | `pypi:git+https://github.com/psf/black.git`      |
| Git branch               | `pypi:git+https://github.com/psf/black.git@main` |

For GitHub sources, `latest` installs from the unpinned default branch; it does
not select the latest published release. Use an explicit version to select a
release. For other Git URLs, `latest` resolves default-branch HEAD to a concrete
commit. Remote tags are also available for explicit version requests.

Direct HTTPS archive URLs are unsupported. Other source syntax may work but is
unsupported and untested.

## Dependency locking

With **uv 0.12.10 or newer**, new `mise.lock` files record the full Python
dependency graph, including wheel hashes and Python/platform markers. Locked
installs reuse that graph without resolving dependencies again.

Create a lockfile, or upgrade an existing version-only lockfile, then install:

```sh
mise lock --upgrade
mise install --locked
```

Commit both `mise.lock` and its [dependency sidecar directory](../mise-lock.md#native-dependency-sidecars),
which contains the native `pyproject.toml` and `uv.lock` files.
Existing lockfiles keep version-only behavior until explicitly upgraded.

### Updating dependencies

Ordinary `mise lock` reuses the recorded graph. To refresh a tool's transitive
dependencies even when its own version has not changed:

```sh
mise lock --bump pypi:black
```

You can also inspect or edit a sidecar with uv. For a tool locked to Black 24.10.0
in the default sidecar layout:

```sh
uv tree --project .mise/locks/pypi-black/24.10.0
uv lock --project .mise/locks/pypi-black/24.10.0 --upgrade-package click
mise lock
```

Run `mise lock` after editing a sidecar to accept its updated digest before using
`mise install --locked`.

### Requirements and limitations

- **Wheels only for dependency graphs:** every dependency needs a published wheel
  for the target Python version and platform. Explicit `mise lock` and locked
  installs do not build source distributions. An ordinary `mise install` falls
  back to a version-only uv installation when it cannot produce a wheel-only graph.
- **PyPI packages with uv:** Git sources and standalone pipx installs use
  version-only locking. pipx cannot replay a uv dependency graph.
- **No free-form installer arguments in dependency graphs:** `uvx_args` and
  `pipx_args` use version-only installation during ordinary installs. Explicit
  dependency locking rejects them because mise cannot safely translate arbitrary
  installer arguments into a reproducible graph. Use the semantic options instead
  when dependency locking is required: [`with`](#with) injects additional
  requirements into the tool environment, [`expose`](#expose) also exposes their
  executables, and [`dependency_prereleases`](#dependency_prereleases) sets uv's
  prerelease policy. These are locked together with the tool, so the graph covers
  the injected packages. Configure [Python](#choosing-python) and the
  [registry URL](#registry-url) directly as well.
- **Installed Python required:** lock generation needs an interpreter discoverable
  by uv, though it need not match the tool's configured Python version. Graph
  installs use the selected mise Python and do not download a replacement.
- **Complete lockfiles required:** revision-2 locked uv installs fail if their
  dependency graph is missing. Run `mise lock` to generate it.

The graph covers the Python range that every locked requirement supports,
starting at Python 3.8, and retains all published wheel targets for portability.
Requirements injected with [`with`](#with) or [`expose`](#expose) are part of
that calculation: one pinned to an exact version raises the range's floor to the
release's own `requires-python`, because a single release cannot span the wider
range the way uv resolves an unpinned requirement. A pin carrying an environment
marker that tests the interpreter, such as `python_version < "3.12"`, is left
out, since it is simply absent from the versions it excludes. This can make
sidecars large. Frozen installs reuse uv's artifact cache.

Different dependency graphs and configured Python interpreters get separate
installations; `mise ls` still shows the package version. For system Python,
mise records the interpreter's implementation, major/minor version, ABI, and
platform during installation so later commands can find the environment even
if that interpreter is no longer on PATH.

### Private indexes and release age

Simple-only indexes must provide consistent `data-requires-python` metadata
on the selected release's wheel links. Registry credentials belong in the
installer environment or credential provider. URLs containing credentials or
query strings cannot be recorded in the lockfile.

[`minimum_release_age`](/configuration/settings.html#minimum_release_age)
filters transitive dependencies when resolving a graph, not when replaying it.
For version-only installs, mise passes uv's `--exclude-newer` flag
(requires uv 0.2.22 or newer) or pip's `--uploaded-prior-to` flag through pipx.

## Choosing Python

For graph-locked tools, configure the interpreter through mise:

```toml
[tools]
python = "3.14"
uv = "0.12.10"
"pypi:black" = "latest"
```

For legacy version-only installs, the selected installer chooses the interpreter;
`uvx_args` and `pipx_args` can pass installer-specific Python options.

## Python upgrades

If a CLI stops working after changing Python, reinstall it under the intended
Python version. This recreates the tool environment and its dependencies:

```sh
mise install --force pypi:black
mise exec -- black --version
```

Check which Python version is active before reinstalling. Existing virtualenvs
and native extensions do not necessarily remain usable after their interpreter
is removed or changed.

## Using pipx

To use the pipx installer, install Python and pipx, then disable uv for the tool:

```sh
mise use python@3.14 pipx
```

```toml
[tools]
"pypi:ansible" = { version = "latest", uvx = false, expose = [], pipx_args = "--include-deps" }
```

This uses version-only locking. An existing uv dependency graph cannot be
replayed with pipx.

### Compatibility with `pipx:`

The `pipx:` backend name remains supported. Existing configurations do not
need to change. However, `pypi:black` and `pipx:black` are distinct tool
identities: switching prefixes creates a separate installation and lock entry.
mise preserves explicit `pipx:` names in output and lockfiles.

The legacy option names `uvx` and `uvx_args` control uv installation;
they do not mean mise runs the `uvx` command.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="pypi" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `pypi` backend—these
go in `[tools]` in `mise.toml`.

### `registry_url` {#registry-url}

Set the registry URL used to resolve versions for this tool. Include a `{}`
placeholder for the package name. This overrides the `pypi.registry_url` setting
for this tool.

```toml
[tools]
"pypi:my-tool" = { version = "latest", registry_url = "https://packages.example.com/pypi/{}/json" }
```

Dependency locking also derives the install index from this URL. For version-only
installs, configure the install index separately through `uvx_args` or
`pipx_args`. For example, with the pipx installer:

```toml
[tools]
"pypi:my-tool" = { version = "latest", uvx = false, registry_url = "https://packages.example.com/pypi/{}/json", pipx_args = "--pip-args='--index-url https://packages.example.com/pypi/simple'" }
```

### `install_env`

Set environment variables for the installer. mise still
sets the tool directory, bin directory, and configured Python package index
variables after applying `install_env`. For the uv installer, for example:

```toml
[tools]
"pypi:black" = { version = "latest", install_env = { UV_COMPILE_BYTECODE = "1" } }
```

### `extras`

Install optional dependencies (Python package extras).

```toml
[tools]
"pypi:harlequin" = { version = "latest", extras = "postgres,s3" }
# equivalent array form:
# "pypi:harlequin" = { version = "latest", extras = ["postgres", "s3"] }
# extras also work with Git sources:
# "pypi:psf/black" = { version = "latest", extras = ["jupyter"] }
```

When passing extras inline, use mise's `key=value` tool-option syntax:

```bash
mise use 'pypi:psf/black[extras=jupyter]@latest'
```

For Git repositories whose name differs from the Python distribution name, set `package_name` so
mise can build the requirement used to select extras:

```toml
[tools]
"pypi:owner/repository" = { version = "latest", package_name = "distribution", extras = ["feature"] }
```

### `pipx_args`

Additional arguments for `pipx install`. These apply only to version-only installs
using pipx and are unsupported with dependency graphs.

```toml
[tools]
"pypi:ansible" = { version = "latest", uvx = false, expose = [], pipx_args = "--include-deps" }
```

### `with`

Install additional Python requirements in the tool environment. This option
requires uv and participates in dependency locking.

```toml
[tools]
"pypi:azure-cli" = { version = "latest", with = ["pip"] }
```

A requirement pinned to an exact version narrows the locked Python range to the
versions that release supports, so the tool may end up needing a newer
interpreter than it declares on its own. Guard the pin with an interpreter
marker, such as `"legacy==1.0.0; python_version < '3.12'"`, to keep the wider
range when the requirement is only needed on some versions.

### `expose`

Install additional Python requirements and expose their executable entry points.
This option requires uv 0.8.5 or newer and participates in dependency locking.

```toml
[tools]
"pypi:ansible" = { version = "latest", expose = ["ansible-core"] }
```

### `dependency_prereleases`

Set uv's prerelease policy for dependency resolution. Supported values are
`disallow`, `allow`, `if-necessary`, and `explicit`. This option requires uv and
is applied both when generating dependency graphs and during version-only installs.

```toml
[tools]
"pypi:azure-cli" = { version = "latest", dependency_prereleases = "allow" }
```

### `uvx`

Set to `false` to use pipx instead of uv for this tool. This also disables
dependency graph locking and requires pipx to be installed.

```toml
[tools]
"pypi:ansible" = { version = "latest", uvx = false, expose = [] }
```

The empty `expose` list clears Ansible's uv-backed registry default. Clear any
other semantic defaults the same way when overriding a registry tool to use pipx.

### `uvx_args`

Additional arguments for version-only installs using `uv tool install`. These
are unsupported with dependency graphs; `pipx_args` applies only to pipx.

```toml
[tools]
"pypi:ansible-core" = { version = "latest", uvx_args = "--resolution lowest" }
```

Prefer the semantic [`with`](#with), [`expose`](#expose), and
[`dependency_prereleases`](#dependency_prereleases) options when they cover the
desired behavior. Unlike arbitrary arguments, those options support dependency
graphs.

Implementation: [`src/backend/pipx.rs`](https://github.com/jdx/mise/blob/main/src/backend/pipx.rs).
