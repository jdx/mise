---
description: "Install Python command-line applications from PyPI or Git into isolated environments."
---

# pypi backend

The `pypi` backend installs Python command-line applications from PyPI or a Git
repository, each into its own virtual environment with its own dependencies. It
installs with uv, or with pipx when uv is not available. Keep your application's
libraries, such as NumPy and requests, in a project environment managed with uv
or pip.

`pipx:` is another name for this backend: `pipx:black` and `pypi:black` install
the same package. See [compatibility with `pipx:`](#compatibility-with-pipx).

## Requirements

<span id="dependencies"></span>

Install uv, or pipx. Add Python as well: pipx needs one to run, and installs
from a [dependency graph](#dependency-locking) need an installed interpreter
rather than one uv downloads. When `uv`, `pipx` or `python` is in your config,
mise installs it before your Python tools. The [`with`](#with),
[`expose`](#expose) and [`dependency_prereleases`](/dev-tools/backends/pypi.html#dependency-prereleases)
options need uv.

## Usage {#usage}

<span id="quick-start"></span>
<span id="project-configuration"></span>

Install uv, Python and Black in the current project:

```sh
mise use python@3.14 uv pypi:black
mise exec -- black --version
```

This writes the following to `mise.toml`. Add `-g` to `mise use` for your
global config.

```toml
[tools]
python = "3.14"
uv = "latest"
"pypi:black" = "latest"
```

When `mise.lock` records a tool's full dependency graph (see
[dependency locking](#dependency-locking)), mise installs it with
`uv sync --frozen`. Otherwise the lockfile records only the tool's version, a
_version-only_ install, and mise runs `uv tool install`, or `pipx install` when
uv is not available. To choose pipx yourself, see
[using pipx instead of uv](#using-pipx).

## Package sources {#supported-pipx-syntax}

Use `pypi:black` for the PyPI distribution or `pypi:psf/black` for its GitHub
repository. Their releases and installation requirements can differ.

| Source                   | Example                                               |
| ------------------------ | ----------------------------------------------------- |
| PyPI, latest version     | `pypi:black`                                          |
| PyPI, specific version   | `pypi:black@24.3.0`                                   |
| GitHub, latest release   | `pypi:psf/black`                                      |
| GitHub, specific release | `pypi:psf/black@24.3.0`                               |
| Git repository           | `pypi:git+https://github.com/psf/black.git`           |
| Git branch               | `pypi:git+https://github.com/psf/black.git@main`      |
| Git subdirectory         | `pypi:git+https://github.com/o/repo#subdirectory=cli` |

Other forms, including direct HTTPS archive URLs, are not supported.

For GitHub sources, `latest` selects the newest GitHub release, and installs
from the default branch only when the repository has no releases. For other Git
URLs, `latest` resolves the default branch's HEAD to a commit. You can request
any branch, tag or commit explicitly.

### Monorepo subdirectories {#git-subdirectory}

For a package in a subdirectory of a Git repository, add the same
`#subdirectory=` fragment that pip and uv accept. The `.git` suffix is optional,
and the fragment also works with the GitHub shorthand:

```sh
mise use 'pypi:git+https://github.com/runpantheon/ltui#subdirectory=ltui@main'
mise use 'pypi:runpantheon/ltui#subdirectory=jtui@main'
```

Quote the argument so the shell does not treat `#` specially. The fragment is
part of the tool name, so each subdirectory is a separate tool, and the version
still goes after `@`. Other fragment keys pass through unchanged, except that
mise reads `[...]` in a tool name as tool options; use the [`extras`](#extras)
option instead of `#egg=pkg[extra]`.

Versions come from the whole repository, so `latest` selects the newest release
even if the subdirectory did not exist at that tag. Pin a branch or commit when
a repository's releases predate the subdirectory. With [`extras`](#extras), mise
guesses the distribution name from the subdirectory; set
[`package_name`](/dev-tools/backends/pypi.html#package-name) if the name differs.

## Dependency locking

With uv 0.12.10 or newer, `mise lock` records each tool's full dependency graph,
including wheel hashes and Python and platform markers, and
`mise install --locked` replays it without resolving again. Create a lockfile,
or upgrade a version-only one, then install:

```sh
mise lock --upgrade
mise install --locked
```

Commit `mise.lock` together with each tool's sidecar directory, which holds the
native `pyproject.toml` and `uv.lock`; see
[dependency graphs](/dev-tools/mise-lock.html#dependency-graphs). Existing
lockfiles keep version-only installs until you upgrade them.

### Updating dependencies

Ordinary `mise lock` reuses the recorded graph. To refresh a tool's dependencies
when its own version has not changed:

```sh
mise lock --bump pypi:black
```

You can also inspect or edit a sidecar with uv. For a tool locked to Black
24.10.0 in the default sidecar layout:

```sh
uv tree --project .mise/locks/pypi-black/24.10.0
uv lock --project .mise/locks/pypi-black/24.10.0 --upgrade-package click
mise lock
```

Run `mise lock` after editing a sidecar to accept its new digest before you use
`mise install --locked`.

### Requirements and limitations

A dependency graph needs a published wheel for every dependency on the target
Python and platform. `mise lock` and locked installs never build source
distributions; a plain `mise install` falls back to a version-only uv install
when it cannot produce a wheel-only graph.

Git sources and pipx installs always use version-only locking, and pipx cannot
replay a uv graph. `uvx_args` and `pipx_args` cannot be locked: a plain
`mise install` falls back to version-only, and `mise lock` rejects them. Use
[`with`](#with), [`expose`](#expose) and
[`dependency_prereleases`](/dev-tools/backends/pypi.html#dependency-prereleases) instead, which are locked
with the tool, and configure [Python](#choosing-python) and the
[registry URL](#registry-url) directly.

`mise lock` needs a Python that uv can find, though not necessarily the tool's
configured version. Installs from a graph use the selected mise Python and do
not download another. A locked install from a lockfile in format 2 or later
fails if the tool's graph is missing; run `mise lock` to create it.

The graph covers the Python range that every locked requirement supports,
starting at Python 3.8, and keeps every published wheel target, so sidecars can
be large. Locked installs reuse uv's artifact cache. Different graphs and Python
interpreters get separate installs; `mise ls` still shows the package version.

## Private indexes

<span id="private-indexes-and-release-age"></span>

To use another index for every tool, set
[`pypi.registry_url`](/dev-tools/backends/pypi.html#pypi.registry_url): mise lists versions from it and
passes its simple index to uv and pip for installs. The per-tool
[`registry_url`](#registry-url) option changes version listing and dependency
locking for one tool; its version-only installs also need the index in
`uvx_args` or `pipx_args`. Simple-only indexes must provide
`data-requires-python` metadata on the selected release's wheel links. Keep
registry credentials in the installer's environment or credential provider: a
URL containing credentials or a query string cannot be recorded in the
lockfile.

## Minimum release age

[`minimum_release_age`](/configuration/settings.html#minimum_release_age) filters
the tool's dependencies when mise resolves a graph, not when it replays one. For
version-only installs, mise passes uv's `--exclude-newer` flag (uv 0.2.22 or
newer) or, through pipx, pip's `--uploaded-prior-to` flag.

## Choosing Python

Configure the interpreter in mise:

```toml
[tools]
python = "3.14"
uv = "0.12.10"
"pypi:black" = "latest"
```

Graph-locked installs use the Python configured in mise, or the first one on
`PATH` when mise manages none. For uv version-only installs, mise passes
`--python <mise python>` to `uv tool install` when mise manages Python, so the
tool's environment does not depend on a Python uv downloaded itself; pass your
own `--python` in [`uvx_args`](/dev-tools/backends/pypi.html#uvx-args) to choose another. pipx uses its own
default interpreter; pass `--python` in [`pipx_args`](/dev-tools/backends/pypi.html#pipx-args) to choose one.

## Python upgrades

When [`mise upgrade`](/cli/upgrade.html) upgrades `python`, mise reinstalls every
installed PyPI tool automatically. On Unix, tools installed without a dependency
graph reach mise's Python through its minor-version path (for example
`.../python/3.14/bin/python`), so patch upgrades keep working without a
reinstall. Graph-locked installs, and minor-version changes made some other way,
need a manual reinstall:

```sh
mise install --force pypi:black
mise exec -- black --version
```

Check which Python is active before reinstalling: a tool's environment and its
native extensions stop working when their interpreter is removed or changed.

## Using pipx instead of uv {#using-pipx}

To install a tool with pipx, add Python and pipx and turn uv off for the tool:

```toml
[tools]
python = "3.14"
pipx = "latest"
"pypi:black" = { version = "latest", uvx = false }
```

To use pipx for every Python tool, set [`pypi.uvx = false`](/dev-tools/backends/pypi.html#pypi.uvx). pipx
installs are version-only, and `with`, `expose` and `dependency_prereleases`
need uv. If a registry short name sets one of these (`ansible` sets `expose`),
clear it with an empty value. An explicit identifier such as `pypi:ansible`
does not carry the registry's options, so this applies only to the short name:

```toml
[tools]
ansible = { version = "latest", uvx = false, expose = [], pipx_args = "--include-deps" }
```

### Compatibility with `pipx:`

The `pipx:` prefix still works, and existing configurations do not need to
change. However, `pypi:black` and `pipx:black` are different tools to mise:
switching the prefix installs a separate copy and adds a separate lockfile
entry. mise keeps an explicit `pipx:` name in output and lockfiles.

The option names `uvx` and `uvx_args` control installs with uv; they do not
mean mise runs the `uvx` command.

## Tool options

Set these on the tool's entry in `[tools]`, or inline, as in
`'pypi:psf/black[extras=jupyter]@latest'`. Options every backend accepts are
described under [tool options](/dev-tools/#tool-options).

| Option                   | Installer                 |
| ------------------------ | ------------------------- |
| `registry_url`           | uv and pipx               |
| `install_env`            | uv and pipx               |
| `extras`                 | uv and pipx               |
| `package_name`           | uv and pipx               |
| `with`                   | uv                        |
| `expose`                 | uv 0.8.5 or later         |
| `dependency_prereleases` | uv                        |
| `uvx_args`               | uv, version-only installs |
| `uvx`                    | chooses pipx when `false` |
| `pipx_args`              | pipx                      |

### `registry_url` {#registry-url}

Set the registry URL used to list this tool's versions. Include a `{}`
placeholder for the package name. This overrides the
[`pypi.registry_url`](/dev-tools/backends/pypi.html#pypi.registry_url) setting for this tool.

```toml
[tools]
"pypi:my-tool" = { version = "latest", registry_url = "https://packages.example.com/pypi/{}/json" }
```

Dependency locking also derives the install index from this URL. For
version-only installs, set the install index separately in `uvx_args` or
`pipx_args`. For example, with pipx:

```toml
[tools."pypi:my-tool"]
version = "latest"
uvx = false
registry_url = "https://packages.example.com/pypi/{}/json"
pipx_args = "--pip-args='--index-url https://packages.example.com/pypi/simple'"
```

### `install_env`

Set environment variables for the installer. mise still sets the tool
directory, the bin directory and the configured package index variables after
applying `install_env`. For uv, for example:

```toml
[tools]
"pypi:black" = { version = "latest", install_env = { UV_COMPILE_BYTECODE = "1" } }
```

### `extras`

Install optional dependencies (Python package extras), as a comma-separated
string or an array. Extras also work with Git sources:

```toml
[tools]
"pypi:harlequin" = { version = "latest", extras = ["postgres", "s3"] }
"pypi:psf/black" = { version = "latest", extras = "jupyter" }
```

Inline, use mise's `key=value` option syntax:

```sh
mise use 'pypi:psf/black[extras=jupyter]@latest'
```

### `package_name`

Set the Python distribution name when a Git repository's name differs from it.
mise needs it to build the requirement that selects `extras` from a Git source:

```toml
[tools]
"pypi:owner/repository" = { version = "latest", package_name = "distribution", extras = ["feature"] }
```

### `with`

Install additional Python requirements into the tool's environment. This option
needs uv and is locked with the tool.

```toml
[tools]
"pypi:azure-cli" = { version = "latest", with = ["pip"] }
```

A requirement pinned to an exact version narrows the locked Python range to the
versions that release supports: its own `requires-python` raises the range's
floor, so the tool may need a newer interpreter than it declares on its own.
Guard the pin with an interpreter marker, such as
`"legacy==1.0.0; python_version < '3.12'"`, to keep the wider range when the
requirement is only needed on some versions. mise leaves such a pin out of the
range calculation.

### `expose`

Install additional Python requirements and put their executables on `PATH` as
well. This option needs uv 0.8.5 or newer and is locked with the tool.

```toml
[tools]
"pypi:ansible" = { version = "latest", expose = ["ansible-core"] }
```

### `dependency_prereleases`

Set uv's prerelease policy for dependencies: `disallow`, `allow`,
`if-necessary` or `explicit`. This option needs uv and applies both when
locking a graph and in version-only installs.

```toml
[tools]
"pypi:azure-cli" = { version = "latest", dependency_prereleases = "allow" }
```

### `uvx_args`

Additional arguments for `uv tool install` in version-only installs. They
cannot be used with dependency graphs; prefer [`with`](#with),
[`expose`](#expose) and [`dependency_prereleases`](/dev-tools/backends/pypi.html#dependency-prereleases) when
they cover what you need.

```toml
[tools]
"pypi:ansible-core" = { version = "latest", uvx_args = "--resolution lowest" }
```

### `uvx`

Set to `false` to install this tool with pipx instead of uv. This also turns off
dependency graph locking, and pipx must be installed. See
[using pipx instead of uv](#using-pipx).

### `pipx_args`

Additional arguments for `pipx install`. They apply only to pipx installs and
cannot be used with dependency graphs.

```toml
[tools]
ansible = { version = "latest", uvx = false, expose = [], pipx_args = "--include-deps" }
```

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="pypi" :level="3" />

## Troubleshooting

| Message                                                                  | What to do                                                                                |
| ------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- |
| `<tool> has no uv dependency graph; run mise lock`                       | The lockfile has no graph for the tool. Run `mise lock`, then install again.              |
| `semantic options (with, expose, and dependency_prereleases) require uv` | Add `uv` to `[tools]`, and make sure neither `uvx = false` nor `pypi.uvx = false` is set. |
| `pipx is required to install <tool> but was not found`                   | Add `pipx` to `[tools]`, or `uv` unless the tool sets `uvx = false`.                      |
| A tool stops working after a Python change                               | Reinstall it; see [Python upgrades](#python-upgrades).                                    |

Implementation: [`src/backend/pipx.rs`](https://github.com/jdx/mise/blob/main/src/backend/pipx.rs).
